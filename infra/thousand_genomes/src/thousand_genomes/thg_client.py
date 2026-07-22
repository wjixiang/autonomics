"""连接到 1000 Genomes 公开 S3 bucket 的客户端及常用文件操作。

``BaseClient``（botocore）是所有 AWS 服务客户端的基类。boto3 根据
服务名 ``"s3"`` 动态生成一个继承自 ``BaseClient`` 的客户端，把每个
S3 REST 操作绑成一个同名方法（``list_objects_v2`` / ``get_object`` /
``head_object`` / ``delete_object`` ...），调用即发送对应的 HTTP 请求，
返回解析后的 dict。
"""

from __future__ import annotations

import fnmatch
import os
from collections.abc import Callable, Iterable, Iterator
from concurrent.futures import ThreadPoolExecutor, as_completed
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
# 默认下载根目录，取自环境变量 DOWNLOAD_DIR_PATH；未设置时为 None。
DOWNLOAD_DIR_ENV = "DOWNLOAD_DIR_PATH"
# Phase 3 最终发布前缀（v5a 主集等全量数据所在目录）。
RELEASE_20130502 = "release/20130502/"


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
        download_dir: str | Path | None = None,
        **kwargs,
    ) -> None:
        self.bucket = bucket
        # 默认下载根目录：显式传入优先，否则读环境变量 DOWNLOAD_DIR_PATH。
        self.download_root: Path | None = self._resolve_download_dir(download_dir)
        self.client: BaseClient = self.create_client(
            anonymous=anonymous,
            region_name=region_name,
            endpoint_url=endpoint_url,
            **kwargs,
        )

    @staticmethod
    def _resolve_download_dir(download_dir: str | Path | None) -> Path | None:
        """解析下载根目录：显式参数 > 环境变量 DOWNLOAD_DIR_PATH > None。

        设置时会自动创建该目录（含父目录）。
        """
        raw = download_dir if download_dir is not None else os.environ.get(DOWNLOAD_DIR_ENV)
        if not raw:
            return None
        path = Path(raw).expanduser()
        path.mkdir(parents=True, exist_ok=True)
        return path

    def _default_local_path(self, key: str) -> Path:
        """按 key 的相对结构，在下载根目录下拼出本地路径。

        download_dir 未设置时会报错。
        """
        if self.download_root is None:
            raise ValueError(
                "未指定下载路径：请设置环境变量 "
                f"{DOWNLOAD_DIR_ENV}，或显式传入 local_path/download_dir。"
            )
        # 用 key 的完整相对结构存放，避免不同前缀下重名文件冲突。
        return self.download_root / key

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
    def download_file(self, key: str, local_path: str | Path | None = None) -> Path:
        """把单个对象完整下载到本地文件（托管式，自动分片）。

        底层 ``client.download_file`` —— 适合大文件，不把内容载入内存。

        Parameters
        ----------
        key:
            对象的 S3 key。
        local_path:
            本地保存路径。默认 ``None`` 时按 key 的相对结构落到下载根目录
            （环境变量 ``DOWNLOAD_DIR_PATH`` 指定）。
        """
        local_path = Path(local_path) if local_path is not None else self._default_local_path(key)
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

    # ---- 批量下载内部工具 ---- #
    @staticmethod
    def _key_matches(key: str, include, exclude) -> bool:
        """按 include/exclude glob 模式过滤 key。

        模式同时尝试匹配**完整 key** 和**文件名（basename）**，所以
        ``"README_2014*"`` 能命中 ``release/20130502/README_20140912_...``。
        """
        name = key.rsplit("/", 1)[-1]

        def matches(pats):
            return any(fnmatch.fnmatch(key, p) or fnmatch.fnmatch(name, p) for p in pats)

        if include and not matches(include):
            return False
        if exclude and matches(exclude):
            return False
        return True

    def _download_one(
        self,
        key: str,
        remote_size: int,
        dest: Path,
        *,
        skip_existing: bool,
        transfer_config,
    ) -> dict:
        """下载单个对象，返回结果 dict。已下完（大小一致）则跳过。"""
        if skip_existing and dest.exists() and dest.stat().st_size == remote_size:
            return {"key": key, "status": "skipped", "size": remote_size, "path": dest}
        try:
            dest.parent.mkdir(parents=True, exist_ok=True)
            self.client.download_file(self.bucket, key, str(dest), Config=transfer_config)
            return {"key": key, "status": "downloaded", "size": remote_size, "path": dest}
        except Exception as e:  # 单个失败不影响整体
            return {"key": key, "status": "failed", "size": remote_size, "path": dest, "error": str(e)}

    def download_objects(
        self,
        objects: Iterable[dict | tuple],
        local_dir: str | Path | None = None,
        *,
        strip_prefix: str = "",
        concurrency: int = 8,
        skip_existing: bool = True,
        transfer_config: TransferConfig | None = None,
        on_progress: Callable[[dict, int, int], None] | None = None,
    ) -> dict:
        """并发下载一批对象，支持断点续传（跳过已下完）与进度回调。

        Parameters
        ----------
        objects:
            对象集合，每个元素是 ``list()`` 返回的 dict（含 ``Key``/``Size``），
            或 ``(key, size)`` 元组。
        local_dir:
            本地目标目录。默认 ``None`` 时用下载根目录（``DOWNLOAD_DIR_PATH``）。
        strip_prefix:
            拼本地相对路径时要去掉的前缀（通常等于下载的目录前缀）。
        concurrency:
            并发下载数。
        skip_existing:
            本地已存在且大小一致时跳过（断点续传）。
        transfer_config:
            boto3 传输配置。默认关闭单文件内部多线程（``use_threads=False``），
            改由 ``concurrency`` 在文件间并行，避免线程爆炸。
        on_progress:
            每个文件完成后回调 ``on_progress(result, done, total)``。
        """
        if local_dir is None:
            if self.download_root is None:
                raise ValueError(
                    "未指定下载目录：请设置环境变量 "
                    f"{DOWNLOAD_DIR_ENV}，或显式传入 local_dir。"
                )
            local_dir = self.download_root
        local_dir = Path(local_dir)
        local_dir.mkdir(parents=True, exist_ok=True)

        if transfer_config is None:
            transfer_config = TransferConfig(use_threads=False)

        # 归一化成 (key, size) 列表
        tasks: list[tuple[str, int, Path]] = []
        for obj in objects:
            if isinstance(obj, dict):
                key, size = obj["Key"], obj.get("Size", 0)
            else:
                key, size = obj
            rel = key[len(strip_prefix) :].lstrip("/") if strip_prefix else key
            tasks.append((key, size, local_dir / rel))

        total = len(tasks)
        results: list[dict] = [None] * total  # type: ignore[list-item]
        with ThreadPoolExecutor(max_workers=concurrency) as pool:
            future_to_idx = {
                pool.submit(
                    self._download_one, k, s, d,
                    skip_existing=skip_existing, transfer_config=transfer_config,
                ): i
                for i, (k, s, d) in enumerate(tasks)
            }
            done = 0
            for fut in as_completed(future_to_idx):
                idx = future_to_idx[fut]
                res = fut.result()
                results[idx] = res
                done += 1
                if on_progress is not None:
                    on_progress(res, done, total)

        return self._summarize_results(results)

    @staticmethod
    def _summarize_results(results: list[dict]) -> dict:
        summary = {"downloaded": 0, "skipped": 0, "failed": 0,
                   "bytes_downloaded": 0, "details": results}
        for r in results:
            summary[r["status"]] += 1
            if r["status"] == "downloaded":
                summary["bytes_downloaded"] += r["size"]
        return summary

    def download_dir(
        self,
        prefix: str,
        local_dir: str | Path | None = None,
        *,
        include=None,
        exclude=None,
        dry_run: bool = False,
        concurrency: int = 8,
        skip_existing: bool = True,
        transfer_config: TransferConfig | None = None,
        on_progress: Callable[[dict, int, int], None] | None = None,
    ) -> dict:
        """递归下载某个前缀下的全部对象（并发、可过滤、可续传）。

        Parameters
        ----------
        prefix:
            要下载的 S3 前缀（"目录"）。
        local_dir:
            本地目标目录。默认 ``None`` 时落到下载根目录（``DOWNLOAD_DIR_PATH``）。
        include / exclude:
            对 key 的 glob 过滤模式列表（``fnmatch`` 语法）。例如
            ``include=["*v5a*.genotypes.vcf.gz*"]`` 只下 v5a 基因型 VCF 及其索引。
        dry_run:
            只统计将下载哪些文件及总大小，不真正下载。
        concurrency / skip_existing / transfer_config / on_progress:
            见 :meth:`download_objects`。
        """
        objs = [
            o for o in self.list(prefix)
            if self._key_matches(o["Key"], include, exclude)
        ]
        total_bytes = sum(o.get("Size", 0) for o in objs)
        preview = {
            "prefix": prefix, "count": len(objs), "total_bytes": total_bytes,
            "total_gb": total_bytes / 1e9,
        }
        if dry_run:
            return preview
        if not objs:
            return {**preview, "downloaded": 0, "skipped": 0, "failed": 0}

        return {
            **preview,
            **self.download_objects(
                objs, local_dir, strip_prefix=prefix,
                concurrency=concurrency, skip_existing=skip_existing,
                transfer_config=transfer_config, on_progress=on_progress,
            ),
        }

    def download_genotypes(
        self,
        local_dir: str | Path | None = None,
        *,
        callset: str = "v5a",
        include_index: bool = True,
        dry_run: bool = False,
        **kwargs,
    ) -> dict:
        """下载基因型数据（genotype VCF）。

        默认下载 Phase 3 最终发布的 **v5a 主集**——即算 LD / 群体遗传分析
        通用的那套 phased 基因型（22 条常染色体，~15.5 GB，加索引约 30 GB）。

        Parameters
        ----------
        callset:
            选择哪一套基因型。可选：

            - ``"v5a"``（默认）：主集成集
              ``ALL.chrN.phase3_shapeit2_mvncall_integrated_v5a.20130502.genotypes.vcf.gz``
            - ``"all"``：发布目录下**所有** ``*.genotypes.vcf.gz``（含 v5b、
              related_samples、lobSTR/microsat 等，体积大很多）
            - 或直接传一个自定义 glob 模式（对文件名匹配）
        include_index:
            是否一并下载配套的 ``.tbi`` 索引（默认 ``True``，按区间查询需要）。
        dry_run:
            只统计将下载哪些文件及总大小，不真正下载。
        **kwargs:
            透传给 :meth:`download_dir`（如 ``concurrency``、``skip_existing``、
            ``on_progress``）。
        """
        presets = {
            "v5a": "*integrated_v5a.20130502.genotypes.vcf.gz",
            "all": "*.genotypes.vcf.gz",
        }
        pattern = presets.get(callset, callset)
        if include_index:
            # 末尾加 * 即可同时命中 .vcf.gz 与 .vcf.gz.tbi
            pattern = pattern + "*"
        return self._download_release_20130502(
            local_dir, include=[pattern], dry_run=dry_run, **kwargs,
        )

    def _download_release_20130502(
        self,
        local_dir: str | Path | None = None,
        *,
        include=None,
        exclude=None,
        dry_run: bool = False,
        **kwargs,
    ) -> dict:
        """下载 Phase 3 最终发布（``release/20130502/``）的内部实现。

        ⚠️ 全量约 **2.87 TB / 1976 个文件**，含 PSMC、lobSTR、SHAPEIT 等附加产物。
        对外请优先使用公开的 :meth:`download_genotypes`；确需全量/自定义子集时
        再用本方法，并建议先用 ``dry_run=True`` 预览，或用 ``include``/``exclude``
        收窄范围，例如：

        - 排除最大的几类附加数据::

              exclude=["*.haps.gz", "*.tar.gz", "*microsat*", "*lobSTR*"]

        Parameters
        ----------
        其余参数同 :meth:`download_dir`。
        """
        return self.download_dir(
            RELEASE_20130502, local_dir,
            include=include, exclude=exclude, dry_run=dry_run, **kwargs,
        )


# 模块级共享客户端（匿名、连真 AWS、指向 1000genomes bucket）。
client = ThgClient()
s3_client: BaseClient = client.client
