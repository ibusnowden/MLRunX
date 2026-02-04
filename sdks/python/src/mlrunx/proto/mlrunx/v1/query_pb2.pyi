import datetime

from google.protobuf import timestamp_pb2 as _timestamp_pb2
from mlrunx.v1 import common_pb2 as _common_pb2
from mlrunx.v1 import ingest_pb2 as _ingest_pb2
from google.protobuf.internal import containers as _containers
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class ComparisonOp(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    COMPARISON_OP_UNSPECIFIED: _ClassVar[ComparisonOp]
    COMPARISON_OP_EQ: _ClassVar[ComparisonOp]
    COMPARISON_OP_NE: _ClassVar[ComparisonOp]
    COMPARISON_OP_GT: _ClassVar[ComparisonOp]
    COMPARISON_OP_GE: _ClassVar[ComparisonOp]
    COMPARISON_OP_LT: _ClassVar[ComparisonOp]
    COMPARISON_OP_LE: _ClassVar[ComparisonOp]
    COMPARISON_OP_CONTAINS: _ClassVar[ComparisonOp]

class RunSortField(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    RUN_SORT_FIELD_UNSPECIFIED: _ClassVar[RunSortField]
    RUN_SORT_FIELD_CREATED_AT: _ClassVar[RunSortField]
    RUN_SORT_FIELD_NAME: _ClassVar[RunSortField]
    RUN_SORT_FIELD_STATUS: _ClassVar[RunSortField]
    RUN_SORT_FIELD_DURATION: _ClassVar[RunSortField]

class SortDirection(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    SORT_DIRECTION_UNSPECIFIED: _ClassVar[SortDirection]
    SORT_DIRECTION_ASC: _ClassVar[SortDirection]
    SORT_DIRECTION_DESC: _ClassVar[SortDirection]

class DownsampleMethod(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    DOWNSAMPLE_METHOD_UNSPECIFIED: _ClassVar[DownsampleMethod]
    DOWNSAMPLE_METHOD_LTTB: _ClassVar[DownsampleMethod]
    DOWNSAMPLE_METHOD_MIN_MAX: _ClassVar[DownsampleMethod]
    DOWNSAMPLE_METHOD_AVERAGE: _ClassVar[DownsampleMethod]
    DOWNSAMPLE_METHOD_FIRST: _ClassVar[DownsampleMethod]
    DOWNSAMPLE_METHOD_LAST: _ClassVar[DownsampleMethod]

class AlignmentMode(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    ALIGNMENT_MODE_UNSPECIFIED: _ClassVar[AlignmentMode]
    ALIGNMENT_MODE_STEP: _ClassVar[AlignmentMode]
    ALIGNMENT_MODE_RELATIVE_TIME: _ClassVar[AlignmentMode]
    ALIGNMENT_MODE_ABSOLUTE_TIME: _ClassVar[AlignmentMode]
    ALIGNMENT_MODE_PROGRESS: _ClassVar[AlignmentMode]
COMPARISON_OP_UNSPECIFIED: ComparisonOp
COMPARISON_OP_EQ: ComparisonOp
COMPARISON_OP_NE: ComparisonOp
COMPARISON_OP_GT: ComparisonOp
COMPARISON_OP_GE: ComparisonOp
COMPARISON_OP_LT: ComparisonOp
COMPARISON_OP_LE: ComparisonOp
COMPARISON_OP_CONTAINS: ComparisonOp
RUN_SORT_FIELD_UNSPECIFIED: RunSortField
RUN_SORT_FIELD_CREATED_AT: RunSortField
RUN_SORT_FIELD_NAME: RunSortField
RUN_SORT_FIELD_STATUS: RunSortField
RUN_SORT_FIELD_DURATION: RunSortField
SORT_DIRECTION_UNSPECIFIED: SortDirection
SORT_DIRECTION_ASC: SortDirection
SORT_DIRECTION_DESC: SortDirection
DOWNSAMPLE_METHOD_UNSPECIFIED: DownsampleMethod
DOWNSAMPLE_METHOD_LTTB: DownsampleMethod
DOWNSAMPLE_METHOD_MIN_MAX: DownsampleMethod
DOWNSAMPLE_METHOD_AVERAGE: DownsampleMethod
DOWNSAMPLE_METHOD_FIRST: DownsampleMethod
DOWNSAMPLE_METHOD_LAST: DownsampleMethod
ALIGNMENT_MODE_UNSPECIFIED: AlignmentMode
ALIGNMENT_MODE_STEP: AlignmentMode
ALIGNMENT_MODE_RELATIVE_TIME: AlignmentMode
ALIGNMENT_MODE_ABSOLUTE_TIME: AlignmentMode
ALIGNMENT_MODE_PROGRESS: AlignmentMode

class ListRunsRequest(_message.Message):
    __slots__ = ("project_id", "filters", "sort", "page_size", "page_token", "include_fields")
    PROJECT_ID_FIELD_NUMBER: _ClassVar[int]
    FILTERS_FIELD_NUMBER: _ClassVar[int]
    SORT_FIELD_NUMBER: _ClassVar[int]
    PAGE_SIZE_FIELD_NUMBER: _ClassVar[int]
    PAGE_TOKEN_FIELD_NUMBER: _ClassVar[int]
    INCLUDE_FIELDS_FIELD_NUMBER: _ClassVar[int]
    project_id: _common_pb2.ProjectId
    filters: RunFilters
    sort: RunSort
    page_size: int
    page_token: str
    include_fields: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, project_id: _Optional[_Union[_common_pb2.ProjectId, _Mapping]] = ..., filters: _Optional[_Union[RunFilters, _Mapping]] = ..., sort: _Optional[_Union[RunSort, _Mapping]] = ..., page_size: _Optional[int] = ..., page_token: _Optional[str] = ..., include_fields: _Optional[_Iterable[str]] = ...) -> None: ...

class RunFilters(_message.Message):
    __slots__ = ("statuses", "tags", "name_pattern", "created_after", "created_before", "user_id", "parent_run_id", "param_filters")
    STATUSES_FIELD_NUMBER: _ClassVar[int]
    TAGS_FIELD_NUMBER: _ClassVar[int]
    NAME_PATTERN_FIELD_NUMBER: _ClassVar[int]
    CREATED_AFTER_FIELD_NUMBER: _ClassVar[int]
    CREATED_BEFORE_FIELD_NUMBER: _ClassVar[int]
    USER_ID_FIELD_NUMBER: _ClassVar[int]
    PARENT_RUN_ID_FIELD_NUMBER: _ClassVar[int]
    PARAM_FILTERS_FIELD_NUMBER: _ClassVar[int]
    statuses: _containers.RepeatedScalarFieldContainer[_common_pb2.RunStatus]
    tags: _containers.RepeatedCompositeFieldContainer[_common_pb2.Tag]
    name_pattern: str
    created_after: _timestamp_pb2.Timestamp
    created_before: _timestamp_pb2.Timestamp
    user_id: str
    parent_run_id: str
    param_filters: _containers.RepeatedCompositeFieldContainer[ParameterFilter]
    def __init__(self, statuses: _Optional[_Iterable[_Union[_common_pb2.RunStatus, str]]] = ..., tags: _Optional[_Iterable[_Union[_common_pb2.Tag, _Mapping]]] = ..., name_pattern: _Optional[str] = ..., created_after: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ..., created_before: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ..., user_id: _Optional[str] = ..., parent_run_id: _Optional[str] = ..., param_filters: _Optional[_Iterable[_Union[ParameterFilter, _Mapping]]] = ...) -> None: ...

class ParameterFilter(_message.Message):
    __slots__ = ("name", "op", "value")
    NAME_FIELD_NUMBER: _ClassVar[int]
    OP_FIELD_NUMBER: _ClassVar[int]
    VALUE_FIELD_NUMBER: _ClassVar[int]
    name: str
    op: ComparisonOp
    value: str
    def __init__(self, name: _Optional[str] = ..., op: _Optional[_Union[ComparisonOp, str]] = ..., value: _Optional[str] = ...) -> None: ...

class RunSort(_message.Message):
    __slots__ = ("field", "direction")
    FIELD_FIELD_NUMBER: _ClassVar[int]
    DIRECTION_FIELD_NUMBER: _ClassVar[int]
    field: RunSortField
    direction: SortDirection
    def __init__(self, field: _Optional[_Union[RunSortField, str]] = ..., direction: _Optional[_Union[SortDirection, str]] = ...) -> None: ...

class ListRunsResponse(_message.Message):
    __slots__ = ("runs", "next_page_token", "total_count")
    RUNS_FIELD_NUMBER: _ClassVar[int]
    NEXT_PAGE_TOKEN_FIELD_NUMBER: _ClassVar[int]
    TOTAL_COUNT_FIELD_NUMBER: _ClassVar[int]
    runs: _containers.RepeatedCompositeFieldContainer[Run]
    next_page_token: str
    total_count: int
    def __init__(self, runs: _Optional[_Iterable[_Union[Run, _Mapping]]] = ..., next_page_token: _Optional[str] = ..., total_count: _Optional[int] = ...) -> None: ...

class Run(_message.Message):
    __slots__ = ("run_id", "project_id", "name", "description", "status", "tags", "params", "summary", "system_info", "git_info", "parent_run_id", "user_id", "created_at", "started_at", "finished_at", "duration_seconds", "metric_count", "artifact_count")
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    PROJECT_ID_FIELD_NUMBER: _ClassVar[int]
    NAME_FIELD_NUMBER: _ClassVar[int]
    DESCRIPTION_FIELD_NUMBER: _ClassVar[int]
    STATUS_FIELD_NUMBER: _ClassVar[int]
    TAGS_FIELD_NUMBER: _ClassVar[int]
    PARAMS_FIELD_NUMBER: _ClassVar[int]
    SUMMARY_FIELD_NUMBER: _ClassVar[int]
    SYSTEM_INFO_FIELD_NUMBER: _ClassVar[int]
    GIT_INFO_FIELD_NUMBER: _ClassVar[int]
    PARENT_RUN_ID_FIELD_NUMBER: _ClassVar[int]
    USER_ID_FIELD_NUMBER: _ClassVar[int]
    CREATED_AT_FIELD_NUMBER: _ClassVar[int]
    STARTED_AT_FIELD_NUMBER: _ClassVar[int]
    FINISHED_AT_FIELD_NUMBER: _ClassVar[int]
    DURATION_SECONDS_FIELD_NUMBER: _ClassVar[int]
    METRIC_COUNT_FIELD_NUMBER: _ClassVar[int]
    ARTIFACT_COUNT_FIELD_NUMBER: _ClassVar[int]
    run_id: _common_pb2.RunId
    project_id: _common_pb2.ProjectId
    name: str
    description: str
    status: _common_pb2.RunStatus
    tags: _containers.RepeatedCompositeFieldContainer[_common_pb2.Tag]
    params: _containers.RepeatedCompositeFieldContainer[_common_pb2.Parameter]
    summary: _containers.RepeatedCompositeFieldContainer[_common_pb2.Parameter]
    system_info: _common_pb2.SystemInfo
    git_info: _ingest_pb2.GitInfo
    parent_run_id: str
    user_id: str
    created_at: _timestamp_pb2.Timestamp
    started_at: _timestamp_pb2.Timestamp
    finished_at: _timestamp_pb2.Timestamp
    duration_seconds: float
    metric_count: int
    artifact_count: int
    def __init__(self, run_id: _Optional[_Union[_common_pb2.RunId, _Mapping]] = ..., project_id: _Optional[_Union[_common_pb2.ProjectId, _Mapping]] = ..., name: _Optional[str] = ..., description: _Optional[str] = ..., status: _Optional[_Union[_common_pb2.RunStatus, str]] = ..., tags: _Optional[_Iterable[_Union[_common_pb2.Tag, _Mapping]]] = ..., params: _Optional[_Iterable[_Union[_common_pb2.Parameter, _Mapping]]] = ..., summary: _Optional[_Iterable[_Union[_common_pb2.Parameter, _Mapping]]] = ..., system_info: _Optional[_Union[_common_pb2.SystemInfo, _Mapping]] = ..., git_info: _Optional[_Union[_ingest_pb2.GitInfo, _Mapping]] = ..., parent_run_id: _Optional[str] = ..., user_id: _Optional[str] = ..., created_at: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ..., started_at: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ..., finished_at: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ..., duration_seconds: _Optional[float] = ..., metric_count: _Optional[int] = ..., artifact_count: _Optional[int] = ...) -> None: ...

class GetRunRequest(_message.Message):
    __slots__ = ("run_id", "include_fields")
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    INCLUDE_FIELDS_FIELD_NUMBER: _ClassVar[int]
    run_id: _common_pb2.RunId
    include_fields: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, run_id: _Optional[_Union[_common_pb2.RunId, _Mapping]] = ..., include_fields: _Optional[_Iterable[str]] = ...) -> None: ...

class GetRunResponse(_message.Message):
    __slots__ = ("run",)
    RUN_FIELD_NUMBER: _ClassVar[int]
    run: Run
    def __init__(self, run: _Optional[_Union[Run, _Mapping]] = ...) -> None: ...

class GetMetricsRequest(_message.Message):
    __slots__ = ("run_ids", "metric_names", "min_step", "max_step", "min_time", "max_time", "max_points", "downsample_method")
    RUN_IDS_FIELD_NUMBER: _ClassVar[int]
    METRIC_NAMES_FIELD_NUMBER: _ClassVar[int]
    MIN_STEP_FIELD_NUMBER: _ClassVar[int]
    MAX_STEP_FIELD_NUMBER: _ClassVar[int]
    MIN_TIME_FIELD_NUMBER: _ClassVar[int]
    MAX_TIME_FIELD_NUMBER: _ClassVar[int]
    MAX_POINTS_FIELD_NUMBER: _ClassVar[int]
    DOWNSAMPLE_METHOD_FIELD_NUMBER: _ClassVar[int]
    run_ids: _containers.RepeatedCompositeFieldContainer[_common_pb2.RunId]
    metric_names: _containers.RepeatedScalarFieldContainer[str]
    min_step: int
    max_step: int
    min_time: _timestamp_pb2.Timestamp
    max_time: _timestamp_pb2.Timestamp
    max_points: int
    downsample_method: DownsampleMethod
    def __init__(self, run_ids: _Optional[_Iterable[_Union[_common_pb2.RunId, _Mapping]]] = ..., metric_names: _Optional[_Iterable[str]] = ..., min_step: _Optional[int] = ..., max_step: _Optional[int] = ..., min_time: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ..., max_time: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ..., max_points: _Optional[int] = ..., downsample_method: _Optional[_Union[DownsampleMethod, str]] = ...) -> None: ...

class GetMetricsResponse(_message.Message):
    __slots__ = ("run_metrics", "downsampled", "original_point_count")
    RUN_METRICS_FIELD_NUMBER: _ClassVar[int]
    DOWNSAMPLED_FIELD_NUMBER: _ClassVar[int]
    ORIGINAL_POINT_COUNT_FIELD_NUMBER: _ClassVar[int]
    run_metrics: _containers.RepeatedCompositeFieldContainer[RunMetrics]
    downsampled: bool
    original_point_count: int
    def __init__(self, run_metrics: _Optional[_Iterable[_Union[RunMetrics, _Mapping]]] = ..., downsampled: _Optional[bool] = ..., original_point_count: _Optional[int] = ...) -> None: ...

class RunMetrics(_message.Message):
    __slots__ = ("run_id", "series")
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    SERIES_FIELD_NUMBER: _ClassVar[int]
    run_id: _common_pb2.RunId
    series: _containers.RepeatedCompositeFieldContainer[MetricSeries]
    def __init__(self, run_id: _Optional[_Union[_common_pb2.RunId, _Mapping]] = ..., series: _Optional[_Iterable[_Union[MetricSeries, _Mapping]]] = ...) -> None: ...

class MetricSeries(_message.Message):
    __slots__ = ("name", "points", "stats")
    NAME_FIELD_NUMBER: _ClassVar[int]
    POINTS_FIELD_NUMBER: _ClassVar[int]
    STATS_FIELD_NUMBER: _ClassVar[int]
    name: str
    points: _containers.RepeatedCompositeFieldContainer[_common_pb2.MetricPoint]
    stats: MetricStats
    def __init__(self, name: _Optional[str] = ..., points: _Optional[_Iterable[_Union[_common_pb2.MetricPoint, _Mapping]]] = ..., stats: _Optional[_Union[MetricStats, _Mapping]] = ...) -> None: ...

class MetricStats(_message.Message):
    __slots__ = ("min", "max", "mean", "last", "count")
    MIN_FIELD_NUMBER: _ClassVar[int]
    MAX_FIELD_NUMBER: _ClassVar[int]
    MEAN_FIELD_NUMBER: _ClassVar[int]
    LAST_FIELD_NUMBER: _ClassVar[int]
    COUNT_FIELD_NUMBER: _ClassVar[int]
    min: float
    max: float
    mean: float
    last: float
    count: int
    def __init__(self, min: _Optional[float] = ..., max: _Optional[float] = ..., mean: _Optional[float] = ..., last: _Optional[float] = ..., count: _Optional[int] = ...) -> None: ...

class CompareRunsRequest(_message.Message):
    __slots__ = ("run_ids", "metric_names", "alignment", "max_points")
    RUN_IDS_FIELD_NUMBER: _ClassVar[int]
    METRIC_NAMES_FIELD_NUMBER: _ClassVar[int]
    ALIGNMENT_FIELD_NUMBER: _ClassVar[int]
    MAX_POINTS_FIELD_NUMBER: _ClassVar[int]
    run_ids: _containers.RepeatedCompositeFieldContainer[_common_pb2.RunId]
    metric_names: _containers.RepeatedScalarFieldContainer[str]
    alignment: AlignmentMode
    max_points: int
    def __init__(self, run_ids: _Optional[_Iterable[_Union[_common_pb2.RunId, _Mapping]]] = ..., metric_names: _Optional[_Iterable[str]] = ..., alignment: _Optional[_Union[AlignmentMode, str]] = ..., max_points: _Optional[int] = ...) -> None: ...

class CompareRunsResponse(_message.Message):
    __slots__ = ("run_metrics", "alignment_info")
    RUN_METRICS_FIELD_NUMBER: _ClassVar[int]
    ALIGNMENT_INFO_FIELD_NUMBER: _ClassVar[int]
    run_metrics: _containers.RepeatedCompositeFieldContainer[RunMetrics]
    alignment_info: AlignmentInfo
    def __init__(self, run_metrics: _Optional[_Iterable[_Union[RunMetrics, _Mapping]]] = ..., alignment_info: _Optional[_Union[AlignmentInfo, _Mapping]] = ...) -> None: ...

class AlignmentInfo(_message.Message):
    __slots__ = ("mode", "x_values", "x_label")
    MODE_FIELD_NUMBER: _ClassVar[int]
    X_VALUES_FIELD_NUMBER: _ClassVar[int]
    X_LABEL_FIELD_NUMBER: _ClassVar[int]
    mode: AlignmentMode
    x_values: _containers.RepeatedScalarFieldContainer[float]
    x_label: str
    def __init__(self, mode: _Optional[_Union[AlignmentMode, str]] = ..., x_values: _Optional[_Iterable[float]] = ..., x_label: _Optional[str] = ...) -> None: ...

class ListArtifactsRequest(_message.Message):
    __slots__ = ("run_id", "types", "name_pattern", "page_size", "page_token")
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    TYPES_FIELD_NUMBER: _ClassVar[int]
    NAME_PATTERN_FIELD_NUMBER: _ClassVar[int]
    PAGE_SIZE_FIELD_NUMBER: _ClassVar[int]
    PAGE_TOKEN_FIELD_NUMBER: _ClassVar[int]
    run_id: _common_pb2.RunId
    types: _containers.RepeatedScalarFieldContainer[_common_pb2.ArtifactType]
    name_pattern: str
    page_size: int
    page_token: str
    def __init__(self, run_id: _Optional[_Union[_common_pb2.RunId, _Mapping]] = ..., types: _Optional[_Iterable[_Union[_common_pb2.ArtifactType, str]]] = ..., name_pattern: _Optional[str] = ..., page_size: _Optional[int] = ..., page_token: _Optional[str] = ...) -> None: ...

class ListArtifactsResponse(_message.Message):
    __slots__ = ("artifacts", "next_page_token", "total_count")
    ARTIFACTS_FIELD_NUMBER: _ClassVar[int]
    NEXT_PAGE_TOKEN_FIELD_NUMBER: _ClassVar[int]
    TOTAL_COUNT_FIELD_NUMBER: _ClassVar[int]
    artifacts: _containers.RepeatedCompositeFieldContainer[_common_pb2.ArtifactMetadata]
    next_page_token: str
    total_count: int
    def __init__(self, artifacts: _Optional[_Iterable[_Union[_common_pb2.ArtifactMetadata, _Mapping]]] = ..., next_page_token: _Optional[str] = ..., total_count: _Optional[int] = ...) -> None: ...

class GetArtifactDownloadUrlRequest(_message.Message):
    __slots__ = ("run_id", "artifact_name")
    RUN_ID_FIELD_NUMBER: _ClassVar[int]
    ARTIFACT_NAME_FIELD_NUMBER: _ClassVar[int]
    run_id: _common_pb2.RunId
    artifact_name: str
    def __init__(self, run_id: _Optional[_Union[_common_pb2.RunId, _Mapping]] = ..., artifact_name: _Optional[str] = ...) -> None: ...

class GetArtifactDownloadUrlResponse(_message.Message):
    __slots__ = ("presigned_url", "expires_at", "metadata")
    PRESIGNED_URL_FIELD_NUMBER: _ClassVar[int]
    EXPIRES_AT_FIELD_NUMBER: _ClassVar[int]
    METADATA_FIELD_NUMBER: _ClassVar[int]
    presigned_url: str
    expires_at: _timestamp_pb2.Timestamp
    metadata: _common_pb2.ArtifactMetadata
    def __init__(self, presigned_url: _Optional[str] = ..., expires_at: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ..., metadata: _Optional[_Union[_common_pb2.ArtifactMetadata, _Mapping]] = ...) -> None: ...

class GetProjectStatsRequest(_message.Message):
    __slots__ = ("project_id", "since")
    PROJECT_ID_FIELD_NUMBER: _ClassVar[int]
    SINCE_FIELD_NUMBER: _ClassVar[int]
    project_id: _common_pb2.ProjectId
    since: _timestamp_pb2.Timestamp
    def __init__(self, project_id: _Optional[_Union[_common_pb2.ProjectId, _Mapping]] = ..., since: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ...) -> None: ...

class GetProjectStatsResponse(_message.Message):
    __slots__ = ("total_runs", "runs_by_status", "total_metrics", "total_artifacts", "storage_bytes", "active_users", "stats_since", "stats_until")
    class RunsByStatusEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: int
        def __init__(self, key: _Optional[str] = ..., value: _Optional[int] = ...) -> None: ...
    TOTAL_RUNS_FIELD_NUMBER: _ClassVar[int]
    RUNS_BY_STATUS_FIELD_NUMBER: _ClassVar[int]
    TOTAL_METRICS_FIELD_NUMBER: _ClassVar[int]
    TOTAL_ARTIFACTS_FIELD_NUMBER: _ClassVar[int]
    STORAGE_BYTES_FIELD_NUMBER: _ClassVar[int]
    ACTIVE_USERS_FIELD_NUMBER: _ClassVar[int]
    STATS_SINCE_FIELD_NUMBER: _ClassVar[int]
    STATS_UNTIL_FIELD_NUMBER: _ClassVar[int]
    total_runs: int
    runs_by_status: _containers.ScalarMap[str, int]
    total_metrics: int
    total_artifacts: int
    storage_bytes: int
    active_users: int
    stats_since: _timestamp_pb2.Timestamp
    stats_until: _timestamp_pb2.Timestamp
    def __init__(self, total_runs: _Optional[int] = ..., runs_by_status: _Optional[_Mapping[str, int]] = ..., total_metrics: _Optional[int] = ..., total_artifacts: _Optional[int] = ..., storage_bytes: _Optional[int] = ..., active_users: _Optional[int] = ..., stats_since: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ..., stats_until: _Optional[_Union[datetime.datetime, _timestamp_pb2.Timestamp, _Mapping]] = ...) -> None: ...

class SearchRunsRequest(_message.Message):
    __slots__ = ("project_id", "query", "filters", "page_size", "page_token")
    PROJECT_ID_FIELD_NUMBER: _ClassVar[int]
    QUERY_FIELD_NUMBER: _ClassVar[int]
    FILTERS_FIELD_NUMBER: _ClassVar[int]
    PAGE_SIZE_FIELD_NUMBER: _ClassVar[int]
    PAGE_TOKEN_FIELD_NUMBER: _ClassVar[int]
    project_id: _common_pb2.ProjectId
    query: str
    filters: RunFilters
    page_size: int
    page_token: str
    def __init__(self, project_id: _Optional[_Union[_common_pb2.ProjectId, _Mapping]] = ..., query: _Optional[str] = ..., filters: _Optional[_Union[RunFilters, _Mapping]] = ..., page_size: _Optional[int] = ..., page_token: _Optional[str] = ...) -> None: ...

class SearchRunsResponse(_message.Message):
    __slots__ = ("results", "next_page_token", "total_count")
    RESULTS_FIELD_NUMBER: _ClassVar[int]
    NEXT_PAGE_TOKEN_FIELD_NUMBER: _ClassVar[int]
    TOTAL_COUNT_FIELD_NUMBER: _ClassVar[int]
    results: _containers.RepeatedCompositeFieldContainer[SearchResult]
    next_page_token: str
    total_count: int
    def __init__(self, results: _Optional[_Iterable[_Union[SearchResult, _Mapping]]] = ..., next_page_token: _Optional[str] = ..., total_count: _Optional[int] = ...) -> None: ...

class SearchResult(_message.Message):
    __slots__ = ("run", "score", "highlights")
    RUN_FIELD_NUMBER: _ClassVar[int]
    SCORE_FIELD_NUMBER: _ClassVar[int]
    HIGHLIGHTS_FIELD_NUMBER: _ClassVar[int]
    run: Run
    score: float
    highlights: _containers.RepeatedScalarFieldContainer[str]
    def __init__(self, run: _Optional[_Union[Run, _Mapping]] = ..., score: _Optional[float] = ..., highlights: _Optional[_Iterable[str]] = ...) -> None: ...
