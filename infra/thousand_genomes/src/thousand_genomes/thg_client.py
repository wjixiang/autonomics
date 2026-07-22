"""连接到 1000 Genomes 公开 S3 bucket 的客户端及常用文件操作。

``BaseClient``（botocore）是所有 AWS 服务客户端的基类。boto3 根据
服务名 ``"s3"`` 动态生成一个继承自 ``BaseClient`` 的客户端，把每个
S3 REST 操作绑成一个同名方法（``list_objects_v2`` / ``get_object`` /
``head_object`` / ``delete_object`` ...），调用即发送对应的 HTTP 请求，
返回解析后的 dict。
"""

from __future__ import annotations

from collections.abc import Iterator
from pathlib import Path

import boto3
from boto3.s3.transfer import TransferConfig
from botocore import UNSIGNED
from botocore.client import BaseClient
from botocore.config import Config

# 1000 Genomes 数据所在的公开 bucket 及其 AWS 区域。
BUCKET = "1000genomes"
REGION = "us-east-1"
# 显式指定 AWS 官方 S3 端点，绕开 ~/.aws/config 中的全局 endpoint_url 覆盖。
ENDPOINT_URL = "https://s3.amazonaws.com"


class ThgClient:
    """1000 Genomes S3 客户端的薄封装。

    把裸 ``BaseClient`` 上常用的 S3 文件操作包装成更顺手的方法：
    列举 / 检查 / 下载 / 读文本。所有方法底层都委托给
    ``self.client``（一个 ``BaseClient``）。
    """

    def __init__(
        self,
        *,
        bucket: str = BUCKET,
        anonymous: bool = True,
        region_name: str = REGION,
        endpoint_url: str | None = ENDPOINT_URL,
        **kwargs,
    ) -> None:
        self.bucket = bucket
        self.client: BaseClient = self.create_client(
            anonymous=anonymous,
            region_name=region_name,
            endpoint_url=endpoint_url,
            **kwargs,
        )

    @staticmethod
    def create_client(
        *,
        anonymous: bool = True,
        region_name: str = REGION,
        endpoint_url: str | None = ENDPOINT_URL,
        **kwargs,
    ) -> BaseClient:
        """创建一个 boto3 S3 ``BaseClient``。

        Parameters
        ----------
        anonymous:
            是否以匿名（未签名）方式访问。该 bucket 公开，默认 ``True``。
            若本机 ``~/.aws/credentials`` 配的是其他 S3 兼容服务（如阿里云）
            的 key，签名请求会失败，此时应保持匿名。
        region_name:
            bucket 所在区域，默认 ``us-east-1``。
        endpoint_url:
            显式端点。默认 AWS 官方端点，绕开配置文件中的第三方端点覆盖。
            传 ``None`` 则回退到 botocore 默认行为。
        **kwargs:
            透传给 ``boto3.client`` 的额外参数。
        """
        if anonymous:
            config = Config(
                signature_version=UNSIGNED,  # 不签名，纯公开读取
                region_name=region_name,
            )
        else:
            config = Config(region_name=region_name)

        return boto3.client(
            "s3",
            region_name=region_name,
            endpoint_url=endpoint_url,
            config=config,
            **kwargs,
        )

    # ------------------------------------------------------------------ #
    # 列举
    # ------------------------------------------------------------------ #
    def list(
        self,
        prefix: str = "",
        *,
        delimiter: str = "",
        page_size: int = 1000,
        max_items: int | None = None,
    ) -> Iterator[dict]:
        """列举对象（自动翻页）。

        底层用 ``client.get_paginator("list_objects_v2")``，
        逐页 yield 每个对象的元数据 dict（含 ``Key``、``Size``、``LastModified``）。
        设 ``delimiter="/"`` 可按"目录"层级列举，此时只 yield 公共前缀。
        """
        paginator = self.client.get_paginator("list_objects_v2")
        pages = paginator.paginate(
            Bucket=self.bucket,
            Prefix=prefix,
            Delimiter=delimiter,
            PaginationConfig={"PageSize": page_size, "MaxItems": max_items},
        )
        for page in pages:
            for obj in page.get("Contents", []):
                yield obj

    def list_prefixes(self, prefix: str = "") -> Iterator[str]:
        """只列举下一级"子目录"（CommonPrefixes），不递归到文件。"""
        paginator = self.client.get_paginator("list_objects_v2")
        for page in paginator.paginate(
            Bucket=self.bucket, Prefix=prefix, Delimiter="/"
        ):
            for cp in page.get("CommonPrefixes", []):
                yield cp["Prefix"]

    # ------------------------------------------------------------------ #
    # 元数据 / 存在性
    # ------------------------------------------------------------------ #
    def stat(self, key: str) -> dict:
        """返回对象元数据（大小、类型、修改时间等），不下载正文。

        底层 ``client.head_object`` —— 发 HEAD 请求。
        对象不存在时会抛 ``ClientError`` (404)。
        """
        return self.client.head_object(Bucket=self.bucket, Key=key)

    def exists(self, key: str) -> bool:
        """对象是否存在（捕获 404）。"""
        from botocore.exceptions import ClientError

        try:
            self.client.head_object(Bucket=self.bucket, Key=key)
            return True
        except ClientError as e:
            if e.response["Error"]["Code"] in {"404", "NoSuchKey"}:
                return False
            raise

    # ------------------------------------------------------------------ #
    # 下载
    # ------------------------------------------------------------------ #
    def download_file(self, key: str, local_path: str | Path) -> Path:
        """把对象完整下载到本地文件（托管式，自动分片/断点续传）。

        底层 ``client.download_file`` —— 适合大文件，不把内容载入内存。
        """
        local_path = Path(local_path)
        local_path.parent.mkdir(parents=True, exist_ok=True)
        self.client.download_file(self.bucket, key, str(local_path))
        return local_path

    def read_bytes(self, key: str) -> bytes:
        """把对象读进内存返回 bytes。

        底层 ``client.get_object`` 拿到流式 body，再 ``read()``。
        适合小文件（如 .tsv 索引、README）。
        """
        body = self.client.get_object(Bucket=self.bucket, Key=key)["Body"]
        return body.read()

    def read_text(self, key: str, encoding: str = "utf-8") -> str:
        """把对象读进内存返回字符串。"""
        return self.read_bytes(key).decode(encoding)

    def open(self, key: str, *, chunk_size: int = 1 << 20):
        """返回可迭代的字节流，用于流式处理大文件而不一次性载入内存。

        底层 ``client.get_object`` 返回的 ``StreamingBody``。
        """
        return self.client.get_object(Bucket=self.bucket, Key=key)["Body"]

    def download_dir(
        self, prefix: str, local_dir: str | Path, *, transfer_config=None
    ) -> list[Path]:
        """递归下载某个前缀下的全部对象到本地目录。

        用 ``list(prefix)`` 拿到所有 key，逐个 ``download_file``。
        """
        local_dir = Path(local_dir)
        local_dir.mkdir(parents=True, exist_ok=True)
        if transfer_config is None:
            transfer_config = TransferConfig()

        downloaded: list[Path] = []
        for obj in self.list(prefix):
            key = obj["Key"]
            # 去掉前缀，拼成本地相对路径
            rel = key[len(prefix) :].lstrip("/")
            dest = local_dir / rel
            self.client.download_file(
                self.bucket, key, str(dest), Config=transfer_config
            )
            downloaded.append(dest)
        return downloaded


# 模块级共享客户端（匿名、连真 AWS、指向 1000genomes bucket）。
client = ThgClient()
s3_client: BaseClient = client.client
print([client.list().__next__() for i in range(10)])
