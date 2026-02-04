import datetime

from google.protobuf import timestamp_pb2 as _timestamp_pb2
from mlrunx.v1 import common_pb2 as _common_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class InitRunRequest(_message.Message):
    __slots__ = ("project_id", "run_id", "name", "description", "tags", "system_info", "git_info", "parent_run_id", "resume_token")
    PROJECT_ID_FIELD_NUMBER: _ClassVar[int]
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    DESCRIPTION_FIELD_NUMBER: _ClassVar[int]
    TAGS_FIELD_NUMBER: _ClassVar[int]
    SYSTEM_INFO_FIELD_NUMBER: _ClassVar[int]
    GIT_INFO_FIELD_NUMBER: _ClassVar[int]
    PARENT_RUN_ID_FIELD_NUMBER: _ClassVar[int]
    RESUME_TOKEN_FIELD_NUMBER: _ClassVar[int]
    project_id: _common_pb2.ProjectId
    run_id: str
    name: str
    description: str
    tags: _containers.RepeatedCompositeFieldContainer[_common_pb2.Tag]
    system_info: _common_pb2.SystemInfo
    git_info: GitInfo
    parent_run_id: str
    resume_token: str
    def __init__(self, project_id: _Optional[_Union[_common_pb2.ProjectId, _Mapping]] = ..., run_id: _Optional[str] = ..., name: _Optional[str] = ..., description: _Optional[str] = ..., tags: _Optional[_Iterable[_Union[_common_pb2.Tag, _Mapping]]] = ..., system_info: _Optional[_Union[_common_pb2.SystemInfo, _Mapping]] = ..., git_info: _Optional[_Union[GitInfo, _Mapping]] = ..., parent_run_id: _Optional[str] = ..., resume_token: _Optional[str] = ...) -> None: ...

class GitInfo(_message.Message):
    __slots__ = ("remote_url", "branch", "commit", "dirty")
    REMOTE_URL_FIELD_NUMBER: _ClassVar[int]
    BRANCH_FIELD_NUMBER: _ClassVar[int]
    COMMIT_FIELD_NUMBER: _ClassVar[int]
    DIRTY_FIELD_NUMBER: _ClassVar[int]
    remote_url: str
    branch: str
    commit: str
    dirty: bool
    def __init__(self, remote_url: _Optional[str] = ..., branch: _Optional[str] = ..., commit: _Optional[str] = ..., dirty: _Optional[bool] = ...) -> None: ...

class InitRunResponse(_message.Message):
    __slots__ = ("run_id", "resume_token", "server_time", "resumed", "warnings")
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    RESUME_TOKEN_FIELD_NUMBER: _ClassVar[int]
    SERVER_TIME_FIELD_NUMBER: _ClassVar[int]
    RESUMED_FIELD_NUMBER: _ClassVar[int]
    WARNINGS_FIELD_NUMBER: _ClassVar[int]
    run_id: _common_pb2.RunId
    resume_token: str
    server_time: _timestamp_pb2.Timestamp
    resumed: bool
    warnings: _containers.RepeatedCompositeFieldContainer[_common_pb2.ErrorDetail]
    def __init__(self, run_id: _Optional[_Union[_common_pb2.RunId, _Mapping]] = ..., resume_token: _Optional[str] = ..., server_time: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ..., resumed: _Optional[bool] = ..., warnings: _Optional[_Iterable[_Union[_common_pb2.ErrorDetail, _Mapping]]] = ...) -> None: ...

class LogMetricsRequest(_message.Message):
    __slots__ = ("run_id", "metrics", "batch_id", "sequence")
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    METRICS_FIELD_NUMBER: _ClassVar[int]
    BATCH_ID_FIELD_NUMBER: _ClassVar[int]
    SEQUENCE_FIELD_NUMBER: _ClassVar[int]
    run_id: _common_pb2.RunId
    metrics: _common_pb2.MetricBatch
    batch_id: str
    sequence: int
    def __init__(self, run_id: _Optional[_Union[_common_pb2.RunId, _Mapping]] = ..., metrics: _Optional[_Union[_common_pb2.MetricBatch, _Mapping]] = ..., batch_id: _Optional[str] = ..., sequence: _Optional[int] = ...) -> None: ...

class LogMetricsResponse(_message.Message):
    __slots__ = ("accepted_count", "deduplicated_count", "warnings", "server_time")
    ACCEPTED_COUNT_FIELD_NUMBER: _ClassVar[int]
    DEDUPLICATED_COUNT_FIELD_NUMBER: _ClassVar[int]
    WARNINGS_FIELD_NUMBER: _ClassVar[int]
    SERVER_TIME_FIELD_NUMBER: _ClassVar[int]
    accepted_count: int
    deduplicated_count: int
    warnings: _containers.RepeatedCompositeFieldContainer[_common_pb2.ErrorDetail]
    server_time: _timestamp_pb2.Timestamp
    def __init__(self, accepted_count: _Optional[int] = ..., deduplicated_count: _Optional[int] = ..., warnings: _Optional[_Iterable[_Union[_common_pb2.ErrorDetail, _Mapping]]] = ..., server_time: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ...) -> None: ...

class LogMetricsStreamRequest(_message.Message):
    __slots__ = ("batch",)
    BATCH_FIELD_NUMBER: _ClassVar[int]
    batch: LogMetricsRequest
    def __init__(self, batch: _Optional[_Union[LogMetricsRequest, _Mapping]] = ...) -> None: ...

class LogMetricsStreamResponse(_message.Message):
    __slots__ = ("response",)
    RESPONSE_FIELD_NUMBER: _ClassVar[int]
    response: LogMetricsResponse
    def __init__(self, response: _Optional[_Union[LogMetricsResponse, _Mapping]] = ...) -> None: ...

class LogParamsRequest(_message.Message):
    __slots__ = ("run_id", "params")
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    PARAMS_FIELD_NUMBER: _ClassVar[int]
    run_id: _common_pb2.RunId
    params: _containers.RepeatedCompositeFieldContainer[_common_pb2.Parameter]
    def __init__(self, run_id: _Optional[_Union[_common_pb2.RunId, _Mapping]] = ..., params: _Optional[_Iterable[_Union[_common_pb2.Parameter, _Mapping]]] = ...) -> None: ...

class LogParamsResponse(_message.Message):
    __slots__ = ("accepted_count", "existing_count", "warnings")
    ACCEPTED_COUNT_FIELD_NUMBER: _ClassVar[int]
    EXISTING_COUNT_FIELD_NUMBER: _ClassVar[int]
    WARNINGS_FIELD_NUMBER: _ClassVar[int]
    accepted_count: int
    existing_count: int
    warnings: _containers.RepeatedCompositeFieldContainer[_common_pb2.ErrorDetail]
    def __init__(self, accepted_count: _Optional[int] = ..., existing_count: _Optional[int] = ..., warnings: _Optional[_Iterable[_Union[_common_pb2.ErrorDetail, _Mapping]]] = ...) -> None: ...

class LogTagsRequest(_message.Message):
    __slots__ = ("run_id", "tags", "remove_keys")
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    TAGS_FIELD_NUMBER: _ClassVar[int]
    REMOVE_KEYS_FIELD_NUMBER: _ClassVar[int]
    run_id: _common_pb2.RunId
    tags: _containers.RepeatedCompositeFieldContainer[_common_pb2.Tag]
    remove_keys: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, run_id: _Optional[_Union[_common_pb2.RunId, _Mapping]] = ..., tags: _Optional[_Iterable[_Union[_common_pb2.Tag, _Mapping]]] = ..., remove_keys: _Optional[_Iterable[str]] = ...) -> None: ...

class LogTagsResponse(_message.Message):
    __slots__ = ("updated_count", "removed_count", "warnings")
    UPDATED_COUNT_FIELD_NUMBER: _ClassVar[int]
    REMOVED_COUNT_FIELD_NUMBER: _ClassVar[int]
    WARNINGS_FIELD_NUMBER: _ClassVar[int]
    updated_count: int
    removed_count: int
    warnings: _containers.RepeatedCompositeFieldContainer[_common_pb2.ErrorDetail]
    def __init__(self, updated_count: _Optional[int] = ..., removed_count: _Optional[int] = ..., warnings: _Optional[_Iterable[_Union[_common_pb2.ErrorDetail, _Mapping]]] = ...) -> None: ...

class CreateArtifactUploadRequest(_message.Message):
    __slots__ = ("run_id", "metadata", "expected_size_bytes")
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    METADATA_FIELD_NUMBER: _ClassVar[int]
    EXPECTED_SIZE_BYTES_FIELD_NUMBER: _ClassVar[int]
    run_id: _common_pb2.RunId
    metadata: _common_pb2.ArtifactMetadata
    expected_size_bytes: int
    def __init__(self, run_id: _Optional[_Union[_common_pb2.RunId, _Mapping]] = ..., metadata: _Optional[_Union[_common_pb2.ArtifactMetadata, _Mapping]] = ..., expected_size_bytes: _Optional[int] = ...) -> None: ...

class CreateArtifactUploadResponse(_message.Message):
    __slots__ = ("upload_id", "presigned_url", "expires_at", "required_headers")
    class RequiredHeadersEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    UPLOAD_ID_FIELD_NUMBER: _ClassVar[int]
    PRESIGNED_URL_FIELD_NUMBER: _ClassVar[int]
    EXPIRES_AT_FIELD_NUMBER: _ClassVar[int]
    REQUIRED_HEADERS_FIELD_NUMBER: _ClassVar[int]
    upload_id: str
    presigned_url: str
    expires_at: _timestamp_pb2.Timestamp
    required_headers: _containers.ScalarMap[str, str]
    def __init__(self, upload_id: _Optional[str] = ..., presigned_url: _Optional[str] = ..., expires_at: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ..., required_headers: _Optional[_Mapping[str, str]] = ...) -> None: ...

class FinalizeArtifactUploadRequest(_message.Message):
    __slots__ = ("upload_id", "actual_size_bytes", "md5_checksum")
    UPLOAD_ID_FIELD_NUMBER: _ClassVar[int]
    ACTUAL_SIZE_BYTES_FIELD_NUMBER: _ClassVar[int]
    MD5_CHECKSUM_FIELD_NUMBER: _ClassVar[int]
    upload_id: str
    actual_size_bytes: int
    md5_checksum: str
    def __init__(self, upload_id: _Optional[str] = ..., actual_size_bytes: _Optional[int] = ..., md5_checksum: _Optional[str] = ...) -> None: ...

class FinalizeArtifactUploadResponse(_message.Message):
    __slots__ = ("artifact", "warnings")
    ARTIFACT_FIELD_NUMBER: _ClassVar[int]
    WARNINGS_FIELD_NUMBER: _ClassVar[int]
    artifact: _common_pb2.ArtifactMetadata
    warnings: _containers.RepeatedCompositeFieldContainer[_common_pb2.ErrorDetail]
    def __init__(self, artifact: _Optional[_Union[_common_pb2.ArtifactMetadata, _Mapping]] = ..., warnings: _Optional[_Iterable[_Union[_common_pb2.ErrorDetail, _Mapping]]] = ...) -> None: ...

class HeartbeatRequest(_message.Message):
    __slots__ = ("run_id", "client_time")
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    CLIENT_TIME_FIELD_NUMBER: _ClassVar[int]
    run_id: _common_pb2.RunId
    client_time: _timestamp_pb2.Timestamp
    def __init__(self, run_id: _Optional[_Union[_common_pb2.RunId, _Mapping]] = ..., client_time: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ...) -> None: ...

class HeartbeatResponse(_message.Message):
    __slots__ = ("server_time", "request_resync")
    SERVER_TIME_FIELD_NUMBER: _ClassVar[int]
    REQUEST_RESYNC_FIELD_NUMBER: _ClassVar[int]
    server_time: _timestamp_pb2.Timestamp
    request_resync: bool
    def __init__(self, server_time: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ..., request_resync: _Optional[bool] = ...) -> None: ...

class FinishRunRequest(_message.Message):
    __slots__ = ("run_id", "status", "exit_code", "error_message", "summary")
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    EXIT_CODE_FIELD_NUMBER: _ClassVar[int]
    ERROR_MESSAGE_FIELD_NUMBER: _ClassVar[int]
    SUMMARY_FIELD_NUMBER: _ClassVar[int]
    run_id: _common_pb2.RunId
    status: _common_pb2.RunStatus
    exit_code: int
    error_message: str
    summary: _containers.RepeatedCompositeFieldContainer[_common_pb2.Parameter]
    def __init__(self, run_id: _Optional[_Union[_common_pb2.RunId, _Mapping]] = ..., status: _Optional[_Union[_common_pb2.RunStatus, str]] = ..., exit_code: _Optional[int] = ..., error_message: _Optional[str] = ..., summary: _Optional[_Iterable[_Union[_common_pb2.Parameter, _Mapping]]] = ...) -> None: ...

class FinishRunResponse(_message.Message):
    __slots__ = ("duration_seconds", "total_metrics", "total_artifacts", "finished_at", "warnings")
    DURATION_SECONDS_FIELD_NUMBER: _ClassVar[int]
    TOTAL_METRICS_FIELD_NUMBER: _ClassVar[int]
    TOTAL_ARTIFACTS_FIELD_NUMBER: _ClassVar[int]
    FINISHED_AT_FIELD_NUMBER: _ClassVar[int]
    WARNINGS_FIELD_NUMBER: _ClassVar[int]
    duration_seconds: float
    total_metrics: int
    total_artifacts: int
    finished_at: _timestamp_pb2.Timestamp
    warnings: _containers.RepeatedCompositeFieldContainer[_common_pb2.ErrorDetail]
    def __init__(self, duration_seconds: _Optional[float] = ..., total_metrics: _Optional[int] = ..., total_artifacts: _Optional[int] = ..., finished_at: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ..., warnings: _Optional[_Iterable[_Union[_common_pb2.ErrorDetail, _Mapping]]] = ...) -> None: ...
