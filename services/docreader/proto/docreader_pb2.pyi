from google.protobuf.internal import containers as _containers
from google.protobuf.internal import enum_type_wrapper as _enum_type_wrapper
from google.protobuf import descriptor as _descriptor
from google.protobuf import message as _message
from collections.abc import Iterable as _Iterable, Mapping as _Mapping
from typing import ClassVar as _ClassVar, Optional as _Optional, Union as _Union

DESCRIPTOR: _descriptor.FileDescriptor

class StructuredSourceUnitKind(int, metaclass=_enum_type_wrapper.EnumTypeWrapper):
    __slots__ = ()
    STRUCTURED_SOURCE_UNIT_KIND_UNSPECIFIED: _ClassVar[StructuredSourceUnitKind]
    STRUCTURED_SOURCE_UNIT_KIND_SECTION: _ClassVar[StructuredSourceUnitKind]
    STRUCTURED_SOURCE_UNIT_KIND_TABLE_ROW: _ClassVar[StructuredSourceUnitKind]
    STRUCTURED_SOURCE_UNIT_KIND_TABLE_REGION: _ClassVar[StructuredSourceUnitKind]
    STRUCTURED_SOURCE_UNIT_KIND_FORM_REGION: _ClassVar[StructuredSourceUnitKind]
    STRUCTURED_SOURCE_UNIT_KIND_ATTACHMENT_REGION: _ClassVar[StructuredSourceUnitKind]
    STRUCTURED_SOURCE_UNIT_KIND_IMAGE_REGION: _ClassVar[StructuredSourceUnitKind]
STRUCTURED_SOURCE_UNIT_KIND_UNSPECIFIED: StructuredSourceUnitKind
STRUCTURED_SOURCE_UNIT_KIND_SECTION: StructuredSourceUnitKind
STRUCTURED_SOURCE_UNIT_KIND_TABLE_ROW: StructuredSourceUnitKind
STRUCTURED_SOURCE_UNIT_KIND_TABLE_REGION: StructuredSourceUnitKind
STRUCTURED_SOURCE_UNIT_KIND_FORM_REGION: StructuredSourceUnitKind
STRUCTURED_SOURCE_UNIT_KIND_ATTACHMENT_REGION: StructuredSourceUnitKind
STRUCTURED_SOURCE_UNIT_KIND_IMAGE_REGION: StructuredSourceUnitKind

class ReadConfig(_message.Message):
    __slots__ = ("parser_engine", "parser_engine_overrides")
    class ParserEngineOverridesEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    PARSER_ENGINE_FIELD_NUMBER: _ClassVar[int]
    PARSER_ENGINE_OVERRIDES_FIELD_NUMBER: _ClassVar[int]
    parser_engine: str
    parser_engine_overrides: _containers.ScalarMap[str, str]
    def __init__(self, parser_engine: _Optional[str] = ..., parser_engine_overrides: _Optional[_Mapping[str, str]] = ...) -> None: ...

class ReadRequest(_message.Message):
    __slots__ = ("file_content", "file_name", "file_type", "url", "title", "config", "request_id")
    FILE_CONTENT_FIELD_NUMBER: _ClassVar[int]
    FILE_NAME_FIELD_NUMBER: _ClassVar[int]
    FILE_TYPE_FIELD_NUMBER: _ClassVar[int]
    URL_FIELD_NUMBER: _ClassVar[int]
    TITLE_FIELD_NUMBER: _ClassVar[int]
    CONFIG_FIELD_NUMBER: _ClassVar[int]
    REQUEST_ID_FIELD_NUMBER: _ClassVar[int]
    file_content: bytes
    file_name: str
    file_type: str
    url: str
    title: str
    config: ReadConfig
    request_id: str
    def __init__(self, file_content: _Optional[bytes] = ..., file_name: _Optional[str] = ..., file_type: _Optional[str] = ..., url: _Optional[str] = ..., title: _Optional[str] = ..., config: _Optional[_Union[ReadConfig, _Mapping]] = ..., request_id: _Optional[str] = ...) -> None: ...

class ImageRef(_message.Message):
    __slots__ = ("filename", "original_ref", "mime_type", "storage_key", "image_data")
    FILENAME_FIELD_NUMBER: _ClassVar[int]
    ORIGINAL_REF_FIELD_NUMBER: _ClassVar[int]
    MIME_TYPE_FIELD_NUMBER: _ClassVar[int]
    STORAGE_KEY_FIELD_NUMBER: _ClassVar[int]
    IMAGE_DATA_FIELD_NUMBER: _ClassVar[int]
    filename: str
    original_ref: str
    mime_type: str
    storage_key: str
    image_data: bytes
    def __init__(self, filename: _Optional[str] = ..., original_ref: _Optional[str] = ..., mime_type: _Optional[str] = ..., storage_key: _Optional[str] = ..., image_data: _Optional[bytes] = ...) -> None: ...

class DocumentLocator(_message.Message):
    __slots__ = ("section_ordinal", "table_ordinal", "row_ordinal", "heading_path", "form_ordinal")
    SECTION_ORDINAL_FIELD_NUMBER: _ClassVar[int]
    TABLE_ORDINAL_FIELD_NUMBER: _ClassVar[int]
    ROW_ORDINAL_FIELD_NUMBER: _ClassVar[int]
    HEADING_PATH_FIELD_NUMBER: _ClassVar[int]
    FORM_ORDINAL_FIELD_NUMBER: _ClassVar[int]
    section_ordinal: int
    table_ordinal: int
    row_ordinal: int
    heading_path: str
    form_ordinal: int
    def __init__(self, section_ordinal: _Optional[int] = ..., table_ordinal: _Optional[int] = ..., row_ordinal: _Optional[int] = ..., heading_path: _Optional[str] = ..., form_ordinal: _Optional[int] = ...) -> None: ...

class PageLocator(_message.Message):
    __slots__ = ("page_ordinal", "left", "top", "right", "bottom")
    PAGE_ORDINAL_FIELD_NUMBER: _ClassVar[int]
    LEFT_FIELD_NUMBER: _ClassVar[int]
    TOP_FIELD_NUMBER: _ClassVar[int]
    RIGHT_FIELD_NUMBER: _ClassVar[int]
    BOTTOM_FIELD_NUMBER: _ClassVar[int]
    page_ordinal: int
    left: float
    top: float
    right: float
    bottom: float
    def __init__(self, page_ordinal: _Optional[int] = ..., left: _Optional[float] = ..., top: _Optional[float] = ..., right: _Optional[float] = ..., bottom: _Optional[float] = ...) -> None: ...

class SpreadsheetCell(_message.Message):
    __slots__ = ("address", "row", "column", "text")
    ADDRESS_FIELD_NUMBER: _ClassVar[int]
    ROW_FIELD_NUMBER: _ClassVar[int]
    COLUMN_FIELD_NUMBER: _ClassVar[int]
    TEXT_FIELD_NUMBER: _ClassVar[int]
    address: str
    row: int
    column: int
    text: str
    def __init__(self, address: _Optional[str] = ..., row: _Optional[int] = ..., column: _Optional[int] = ..., text: _Optional[str] = ...) -> None: ...

class SpreadsheetRange(_message.Message):
    __slots__ = ("a1_range", "start_row", "start_column", "end_row", "end_column")
    A1_RANGE_FIELD_NUMBER: _ClassVar[int]
    START_ROW_FIELD_NUMBER: _ClassVar[int]
    START_COLUMN_FIELD_NUMBER: _ClassVar[int]
    END_ROW_FIELD_NUMBER: _ClassVar[int]
    END_COLUMN_FIELD_NUMBER: _ClassVar[int]
    a1_range: str
    start_row: int
    start_column: int
    end_row: int
    end_column: int
    def __init__(self, a1_range: _Optional[str] = ..., start_row: _Optional[int] = ..., start_column: _Optional[int] = ..., end_row: _Optional[int] = ..., end_column: _Optional[int] = ...) -> None: ...

class SpreadsheetTableIdentity(_message.Message):
    __slots__ = ("name", "display_name", "a1_range")
    NAME_FIELD_NUMBER: _ClassVar[int]
    DISPLAY_NAME_FIELD_NUMBER: _ClassVar[int]
    A1_RANGE_FIELD_NUMBER: _ClassVar[int]
    name: str
    display_name: str
    a1_range: str
    def __init__(self, name: _Optional[str] = ..., display_name: _Optional[str] = ..., a1_range: _Optional[str] = ...) -> None: ...

class SpreadsheetLocator(_message.Message):
    __slots__ = ("sheet_ordinal", "sheet_name", "region", "cells", "merged_ranges", "defined_tables")
    SHEET_ORDINAL_FIELD_NUMBER: _ClassVar[int]
    SHEET_NAME_FIELD_NUMBER: _ClassVar[int]
    REGION_FIELD_NUMBER: _ClassVar[int]
    CELLS_FIELD_NUMBER: _ClassVar[int]
    MERGED_RANGES_FIELD_NUMBER: _ClassVar[int]
    DEFINED_TABLES_FIELD_NUMBER: _ClassVar[int]
    sheet_ordinal: int
    sheet_name: str
    region: SpreadsheetRange
    cells: _containers.RepeatedCompositeFieldContainer[SpreadsheetCell]
    merged_ranges: _containers.RepeatedCompositeFieldContainer[SpreadsheetRange]
    defined_tables: _containers.RepeatedCompositeFieldContainer[SpreadsheetTableIdentity]
    def __init__(self, sheet_ordinal: _Optional[int] = ..., sheet_name: _Optional[str] = ..., region: _Optional[_Union[SpreadsheetRange, _Mapping]] = ..., cells: _Optional[_Iterable[_Union[SpreadsheetCell, _Mapping]]] = ..., merged_ranges: _Optional[_Iterable[_Union[SpreadsheetRange, _Mapping]]] = ..., defined_tables: _Optional[_Iterable[_Union[SpreadsheetTableIdentity, _Mapping]]] = ...) -> None: ...

class ParagraphImageParent(_message.Message):
    __slots__ = ("section_ordinal", "paragraph_ordinal")
    SECTION_ORDINAL_FIELD_NUMBER: _ClassVar[int]
    PARAGRAPH_ORDINAL_FIELD_NUMBER: _ClassVar[int]
    section_ordinal: int
    paragraph_ordinal: int
    def __init__(self, section_ordinal: _Optional[int] = ..., paragraph_ordinal: _Optional[int] = ...) -> None: ...

class TableCellImageParent(_message.Message):
    __slots__ = ("section_ordinal", "table_ordinal", "row_ordinal", "cell_ordinal")
    SECTION_ORDINAL_FIELD_NUMBER: _ClassVar[int]
    TABLE_ORDINAL_FIELD_NUMBER: _ClassVar[int]
    ROW_ORDINAL_FIELD_NUMBER: _ClassVar[int]
    CELL_ORDINAL_FIELD_NUMBER: _ClassVar[int]
    section_ordinal: int
    table_ordinal: int
    row_ordinal: int
    cell_ordinal: int
    def __init__(self, section_ordinal: _Optional[int] = ..., table_ordinal: _Optional[int] = ..., row_ordinal: _Optional[int] = ..., cell_ordinal: _Optional[int] = ...) -> None: ...

class FormImageParent(_message.Message):
    __slots__ = ("section_ordinal", "form_ordinal")
    SECTION_ORDINAL_FIELD_NUMBER: _ClassVar[int]
    FORM_ORDINAL_FIELD_NUMBER: _ClassVar[int]
    section_ordinal: int
    form_ordinal: int
    def __init__(self, section_ordinal: _Optional[int] = ..., form_ordinal: _Optional[int] = ...) -> None: ...

class ImageLocator(_message.Message):
    __slots__ = ("original_ref", "width", "height", "media_type", "page_ordinal", "left", "top", "right", "bottom", "paragraph_parent", "table_cell_parent", "form_parent")
    ORIGINAL_REF_FIELD_NUMBER: _ClassVar[int]
    WIDTH_FIELD_NUMBER: _ClassVar[int]
    HEIGHT_FIELD_NUMBER: _ClassVar[int]
    MEDIA_TYPE_FIELD_NUMBER: _ClassVar[int]
    PAGE_ORDINAL_FIELD_NUMBER: _ClassVar[int]
    LEFT_FIELD_NUMBER: _ClassVar[int]
    TOP_FIELD_NUMBER: _ClassVar[int]
    RIGHT_FIELD_NUMBER: _ClassVar[int]
    BOTTOM_FIELD_NUMBER: _ClassVar[int]
    PARAGRAPH_PARENT_FIELD_NUMBER: _ClassVar[int]
    TABLE_CELL_PARENT_FIELD_NUMBER: _ClassVar[int]
    FORM_PARENT_FIELD_NUMBER: _ClassVar[int]
    original_ref: str
    width: int
    height: int
    media_type: str
    page_ordinal: int
    left: float
    top: float
    right: float
    bottom: float
    paragraph_parent: ParagraphImageParent
    table_cell_parent: TableCellImageParent
    form_parent: FormImageParent
    def __init__(self, original_ref: _Optional[str] = ..., width: _Optional[int] = ..., height: _Optional[int] = ..., media_type: _Optional[str] = ..., page_ordinal: _Optional[int] = ..., left: _Optional[float] = ..., top: _Optional[float] = ..., right: _Optional[float] = ..., bottom: _Optional[float] = ..., paragraph_parent: _Optional[_Union[ParagraphImageParent, _Mapping]] = ..., table_cell_parent: _Optional[_Union[TableCellImageParent, _Mapping]] = ..., form_parent: _Optional[_Union[FormImageParent, _Mapping]] = ...) -> None: ...

class AttachmentLocator(_message.Message):
    __slots__ = ("part_name", "relationship_type")
    PART_NAME_FIELD_NUMBER: _ClassVar[int]
    RELATIONSHIP_TYPE_FIELD_NUMBER: _ClassVar[int]
    part_name: str
    relationship_type: str
    def __init__(self, part_name: _Optional[str] = ..., relationship_type: _Optional[str] = ...) -> None: ...

class StructuredSourceUnit(_message.Message):
    __slots__ = ("key", "ordinal", "kind", "text", "document", "page", "spreadsheet", "image", "attachment")
    KEY_FIELD_NUMBER: _ClassVar[int]
    ORDINAL_FIELD_NUMBER: _ClassVar[int]
    KIND_FIELD_NUMBER: _ClassVar[int]
    TEXT_FIELD_NUMBER: _ClassVar[int]
    DOCUMENT_FIELD_NUMBER: _ClassVar[int]
    PAGE_FIELD_NUMBER: _ClassVar[int]
    SPREADSHEET_FIELD_NUMBER: _ClassVar[int]
    IMAGE_FIELD_NUMBER: _ClassVar[int]
    ATTACHMENT_FIELD_NUMBER: _ClassVar[int]
    key: str
    ordinal: int
    kind: StructuredSourceUnitKind
    text: str
    document: DocumentLocator
    page: PageLocator
    spreadsheet: SpreadsheetLocator
    image: ImageLocator
    attachment: AttachmentLocator
    def __init__(self, key: _Optional[str] = ..., ordinal: _Optional[int] = ..., kind: _Optional[_Union[StructuredSourceUnitKind, str]] = ..., text: _Optional[str] = ..., document: _Optional[_Union[DocumentLocator, _Mapping]] = ..., page: _Optional[_Union[PageLocator, _Mapping]] = ..., spreadsheet: _Optional[_Union[SpreadsheetLocator, _Mapping]] = ..., image: _Optional[_Union[ImageLocator, _Mapping]] = ..., attachment: _Optional[_Union[AttachmentLocator, _Mapping]] = ...) -> None: ...

class ReadResponse(_message.Message):
    __slots__ = ("markdown_content", "image_refs", "image_dir_path", "metadata", "error", "structured_source_units")
    class MetadataEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    MARKDOWN_CONTENT_FIELD_NUMBER: _ClassVar[int]
    IMAGE_REFS_FIELD_NUMBER: _ClassVar[int]
    IMAGE_DIR_PATH_FIELD_NUMBER: _ClassVar[int]
    METADATA_FIELD_NUMBER: _ClassVar[int]
    ERROR_FIELD_NUMBER: _ClassVar[int]
    STRUCTURED_SOURCE_UNITS_FIELD_NUMBER: _ClassVar[int]
    markdown_content: str
    image_refs: _containers.RepeatedCompositeFieldContainer[ImageRef]
    image_dir_path: str
    metadata: _containers.ScalarMap[str, str]
    error: str
    structured_source_units: _containers.RepeatedCompositeFieldContainer[StructuredSourceUnit]
    def __init__(self, markdown_content: _Optional[str] = ..., image_refs: _Optional[_Iterable[_Union[ImageRef, _Mapping]]] = ..., image_dir_path: _Optional[str] = ..., metadata: _Optional[_Mapping[str, str]] = ..., error: _Optional[str] = ..., structured_source_units: _Optional[_Iterable[_Union[StructuredSourceUnit, _Mapping]]] = ...) -> None: ...

class ReadStreamMeta(_message.Message):
    __slots__ = ("markdown_content", "image_dir_path", "metadata", "error", "image_count", "structured_source_units")
    class MetadataEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    MARKDOWN_CONTENT_FIELD_NUMBER: _ClassVar[int]
    IMAGE_DIR_PATH_FIELD_NUMBER: _ClassVar[int]
    METADATA_FIELD_NUMBER: _ClassVar[int]
    ERROR_FIELD_NUMBER: _ClassVar[int]
    IMAGE_COUNT_FIELD_NUMBER: _ClassVar[int]
    STRUCTURED_SOURCE_UNITS_FIELD_NUMBER: _ClassVar[int]
    markdown_content: str
    image_dir_path: str
    metadata: _containers.ScalarMap[str, str]
    error: str
    image_count: int
    structured_source_units: _containers.RepeatedCompositeFieldContainer[StructuredSourceUnit]
    def __init__(self, markdown_content: _Optional[str] = ..., image_dir_path: _Optional[str] = ..., metadata: _Optional[_Mapping[str, str]] = ..., error: _Optional[str] = ..., image_count: _Optional[int] = ..., structured_source_units: _Optional[_Iterable[_Union[StructuredSourceUnit, _Mapping]]] = ...) -> None: ...

class ReadStreamResponse(_message.Message):
    __slots__ = ("meta", "image")
    META_FIELD_NUMBER: _ClassVar[int]
    IMAGE_FIELD_NUMBER: _ClassVar[int]
    meta: ReadStreamMeta
    image: ImageRef
    def __init__(self, meta: _Optional[_Union[ReadStreamMeta, _Mapping]] = ..., image: _Optional[_Union[ImageRef, _Mapping]] = ...) -> None: ...

class ListEnginesRequest(_message.Message):
    __slots__ = ("config_overrides",)
    class ConfigOverridesEntry(_message.Message):
        __slots__ = ("key", "value")
        KEY_FIELD_NUMBER: _ClassVar[int]
        VALUE_FIELD_NUMBER: _ClassVar[int]
        key: str
        value: str
        def __init__(self, key: _Optional[str] = ..., value: _Optional[str] = ...) -> None: ...
    CONFIG_OVERRIDES_FIELD_NUMBER: _ClassVar[int]
    config_overrides: _containers.ScalarMap[str, str]
    def __init__(self, config_overrides: _Optional[_Mapping[str, str]] = ...) -> None: ...

class ParserEngineInfo(_message.Message):
    __slots__ = ("name", "description", "file_types", "available", "unavailable_reason")
    NAME_FIELD_NUMBER: _ClassVar[int]
    DESCRIPTION_FIELD_NUMBER: _ClassVar[int]
    FILE_TYPES_FIELD_NUMBER: _ClassVar[int]
    AVAILABLE_FIELD_NUMBER: _ClassVar[int]
    UNAVAILABLE_REASON_FIELD_NUMBER: _ClassVar[int]
    name: str
    description: str
    file_types: _containers.RepeatedScalarFieldContainer[str]
    available: bool
    unavailable_reason: str
    def __init__(self, name: _Optional[str] = ..., description: _Optional[str] = ..., file_types: _Optional[_Iterable[str]] = ..., available: _Optional[bool] = ..., unavailable_reason: _Optional[str] = ...) -> None: ...

class ListEnginesResponse(_message.Message):
    __slots__ = ("engines",)
    ENGINES_FIELD_NUMBER: _ClassVar[int]
    engines: _containers.RepeatedCompositeFieldContainer[ParserEngineInfo]
    def __init__(self, engines: _Optional[_Iterable[_Union[ParserEngineInfo, _Mapping]]] = ...) -> None: ...
