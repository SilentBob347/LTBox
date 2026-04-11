import sys

from .ota.payload import (
    PayloadPartitionInfo,
    get_old_partition_hashes,
    get_partition_hashes,
    get_partition_infos,
    get_partition_names,
    get_partition_sizes,
    partition_names_from_infos,
)
from .ota import payload as _module

__all__ = [
    "PayloadPartitionInfo",
    "get_old_partition_hashes",
    "get_partition_hashes",
    "get_partition_infos",
    "get_partition_names",
    "get_partition_sizes",
    "partition_names_from_infos",
]

sys.modules[__name__] = _module
