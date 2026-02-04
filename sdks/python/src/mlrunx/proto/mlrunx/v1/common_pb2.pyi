import datetime

from google.protobuf import timestamp_pb2 as _timestamp_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class ArtifactType(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    ARTIFACT_TYPE_UNSPECIFIED: _ClassVar[ArtifactType]
    ARTIFACT_TYPE_MODEL: _ClassVar[ArtifactType]
    ARTIFACT_TYPE_DATASET: _ClassVar[ArtifactType]
    ARTIFACT_TYPE_IMAGE: _ClassVar[ArtifactType]
    ARTIFACT_TYPE_AUDIO: _ClassVar[ArtifactType]
    ARTIFACT_TYPE_VIDEO: _ClassVar[ArtifactType]
    ARTIFACT_TYPE_TABLE: _ClassVar[ArtifactType]
    ARTIFACT_TYPE_CODE: _ClassVar[ArtifactType]
    ARTIFACT_TYPE_OTHER: _ClassVar[ArtifactType]

class RunStatus(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    RUN_STATUS_UNSPECIFIED: _ClassVar[RunStatus]
    RUN_STATUS_RUNNING: _ClassVar[RunStatus]
    RUN_STATUS_FINISHED: _ClassVar[RunStatus]
    RUN_STATUS_FAILED: _ClassVar[RunStatus]
    RUN_STATUS_CRASHED: _ClassVar[RunStatus]
    RUN_STATUS_KILLED: _ClassVar[RunStatus]

class ErrorSeverity(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    ERROR_SEVERITY_UNSPECIFIED: _ClassVar[ErrorSeverity]
    ERROR_SEVERITY_WARNING: _ClassVar[ErrorSeverity]
    ERROR_SEVERITY_ERROR: _ClassVar[ErrorSeverity]
ARTIFACT_TYPE_UNSPECIFIED: ArtifactType
ARTIFACT_TYPE_MODEL: ArtifactType
ARTIFACT_TYPE_DATASET: ArtifactType
ARTIFACT_TYPE_IMAGE: ArtifactType
ARTIFACT_TYPE_AUDIO: ArtifactType
ARTIFACT_TYPE_VIDEO: ArtifactType
ARTIFACT_TYPE_TABLE: ArtifactType
ARTIFACT_TYPE_CODE: ArtifactType
ARTIFACT_TYPE_OTHER: ArtifactType
RUN_STATUS_UNSPECIFIED: RunStatus
RUN_STATUS_RUNNING: RunStatus
RUN_STATUS_FINISHED: RunStatus
RUN_STATUS_FAILED: RunStatus
RUN_STATUS_CRASHED: RunStatus
RUN_STATUS_KILLED: RunStatus
ERROR_SEVERITY_UNSPECIFIED: ErrorSeverity
ERROR_SEVERITY_WARNING: ErrorSeverity
ERROR_SEVERITY_ERROR: ErrorSeverity

class ProjectId(_message.Message):
    __slots__ = ("value",)
    VALUE_FIELD_NUMBER: _ClassVar[int]
    value: str
    def __init__(self, value: _Optional[str] = ...) -> None: ...

class RunId(_message.Message):
    __slots__ = ("value",)
    VALUE_FIELD_NUMBER: _ClassVar[int]
    value: str
    def __init__(self, value: _Optional[str] = ...) -> None: ...

class MetricPoint(_message.Message):
    __slots__ = ("name", "step", "value", "timestamp")
    NAME_FIELD_NUMBER: _ClassVar[int]
    STEP_FIELD_NUMBER: _ClassVar[int]
    VALUE_FIELD_NUMBER: _ClassVar[int]
    TIMESTAMP_FIELD_NUMBER: _ClassVar[int]
    name: str
    step: int
    value: float
    timestamp: _timestamp_pb2.Timestamp
    def __init__(self, name: _Optional[str] = ..., step: _Optional[int] = ..., value: _Optional[float] = ..., timestamp: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ...) -> None: ...

class MetricBatch(_message.Message):
    __slots__ = ("points",)
    POINTS_FIELD_NUMBER: _ClassVar[int]
    points: _containers.RepeatedCompositeFieldContainer[MetricPoint]
    def __init__(self, points: _Optional[_Iterable[_Union[MetricPoint, _Mapping]]] = ...) -> None: ...

class Parameter(_message.Message):
    __slots__ = ("name", "value")
    NAME_FIELD_NUMBER: _ClassVar[int]
    VALUE_FIELD_NUMBER: _ClassVar[int]
    name: str
    value: str
    def __init__(self, name: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...

class Tag(_message.Message):
    __slots__ = ("key", "value")
    KEY_FIELD_NUMBER: _ClassVar[int]
    VALUE_FIELD_NUMBER: _ClassVar[int]
    key: str
    value: str
    def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...

class ArtifactMetadata(_message.Message):
    __slots__ = ("name", "type", "mime_type", "size_bytes", "md5_checksum", "step", "created_at")
    NAME_FIELD_NUMBER: _ClassVar[int]
    TYPE_FIELD_NUMBER: _ClassVar[int]
    MIME_TYPE_FIELD_NUMBER: _ClassVar[int]
    SIZE_BYTES_FIELD_NUMBER: _ClassVar[int]
    MD5_CHECKSUM_FIELD_NUMBER: _ClassVar[int]
    STEP_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    name: str
    type: ArtifactType
    mime_type: str
    size_bytes: int
    md5_checksum: str
    step: int
    created_at: _timestamp_pb2.Timestamp
    def __init__(self, name: _Optional[str] = ..., type: _Optional[_Union[ArtifactType, str]] = ..., mime_type: _Optional[str] = ..., size_bytes: _Optional[int] = ..., md5_checksum: _Optional[str] = ..., step: _Optional[int] = ..., created_at: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ...) -> None: ...

class SystemInfo(_message.Message):
    __slots__ = ("hostname", "os", "python_version", "gpus", "cpu_count", "memory_bytes")
    HOSTNAME_FIELD_NUMBER: _ClassVar[int]
    OS_FIELD_NUMBER: _ClassVar[int]
    PYTHON_VERSION_FIELD_NUMBER: _ClassVar[int]
    GPUS_FIELD_NUMBER: _ClassVar[int]
    CPU_COUNT_FIELD_NUMBER: _ClassVar[int]
    MEMORY_BYTES_FIELD_NUMBER: _ClassVar[int]
    hostname: str
    os: str
    python_version: str
    gpus: _containers.RepeatedCompositeFieldContainer[GpuInfo]
    cpu_count: int
    memory_bytes: int
    def __init__(self, hostname: _Optional[str] = ..., os: _Optional[str] = ..., python_version: _Optional[str] = ..., gpus: _Optional[_Iterable[_Union[GpuInfo, _Mapping]]] = ..., cpu_count: _Optional[int] = ..., memory_bytes: _Optional[int] = ...) -> None: ...

class GpuInfo(_message.Message):
    __slots__ = ("index", "name", "memory_bytes", "cuda_version")
    INDEX_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    MEMORY_BYTES_FIELD_NUMBER: _ClassVar[int]
    CUDA_VERSION_FIELD_NUMBER: _ClassVar[int]
    index: int
    name: str
    memory_bytes: int
    cuda_version: str
    def __init__(self, index: _Optional[int] = ..., name: _Optional[str] = ..., memory_bytes: _Optional[int] = ..., cuda_version: _Optional[str] = ...) -> None: ...

class ErrorDetail(_message.Message):
    __slots__ = ("code", "message", "field", "severity")
    CODE_FIELD_NUMBER: _ClassVar[int]
    MESSAGE_FIELD_NUMBER: _ClassVar[int]
    FIELD_FIELD_NUMBER: _ClassVar[int]
    SEVERITY_FIELD_NUMBER: _ClassVar[int]
    code: str
    message: str
    field: str
    severity: ErrorSeverity
    def __init__(self, code: _Optional[str] = ..., message: _Optional[str] = ..., field: _Optional[str] = ..., severity: _Optional[_Union[ErrorSeverity, str]] = ...) -> None: ...

class PageToken(_message.Message):
    __slots__ = ("cursor",)
    CURSOR_FIELD_NUMBER: _ClassVar[int]
    cursor: str
    def __init__(self, cursor: _Optional[str] = ...) -> None: ...
