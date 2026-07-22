"""1000 Genomes Project 数据访问工具。

数据托管在公开 AWS S3 bucket ``s3://1000genomes``（区域 us-east-1），
可通过 HTTPS 直接访问，也可用 boto3 列举 / 下载。

注意：本机 ``~/.aws/config`` 可能配置了指向其他 S3 兼容服务（如阿里云 OSS）
的全局 ``endpoint_url``。本模块默认显式使用 AWS 官方 S3 端点，避免被覆盖。
"""

from __future__ import annotations


def hello() -> str:
    return "Hello from thousand_genomes!"
