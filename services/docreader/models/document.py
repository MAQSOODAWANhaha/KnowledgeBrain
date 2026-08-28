"""Chunk document schema."""

import json
from enum import Enum
from typing import Annotated, Any, Dict, List, Literal, Optional, Union

from pydantic import BaseModel, Field, model_validator


class Chunk(BaseModel):
    """Document Chunk including chunk content, chunk metadata."""

    content: str = Field(default="", description="chunk text content")
    seq: int = Field(default=0, description="Chunk sequence number")
    start: int = Field(default=0, description="Chunk start position")
    end: int = Field(description="Chunk end position")
    images: List[Dict[str, Any]] = Field(
        default_factory=list, description="Images in the chunk"
    )

    metadata: Dict[str, Any] = Field(
        default_factory=dict,
        description="metadata fields",
    )

    def to_dict(self, **kwargs: Any) -> Dict[str, Any]:
        """Convert Chunk to dict."""

        data = self.model_dump()
        data.update(kwargs)
        data["class_name"] = self.__class__.__name__
        return data

    def to_json(self, **kwargs: Any) -> str:
        """Convert Chunk to json."""
        data = self.to_dict(**kwargs)
        return json.dumps(data)

    def __hash__(self):
        """Hash function."""
        return hash((self.content,))

    def __eq__(self, other):
        """Equal function."""
        return self.content == other.content

    @classmethod
    def from_dict(cls, data: Dict[str, Any], **kwargs: Any) -> "Chunk":
        """Create Chunk from dict."""
        if isinstance(kwargs, dict):
            data.update(kwargs)

        data.pop("class_name", None)
        return cls(**data)

    @classmethod
    def from_json(cls, data_str: str, **kwargs: Any) -> "Chunk":
        """Create Chunk from json."""
        data = json.loads(data_str)
        return cls.from_dict(data, **kwargs)


class StructuredSourceUnitKind(str, Enum):
    SECTION = "section"
    TABLE_ROW = "table_row"
    TABLE_REGION = "table_region"
    FORM_REGION = "form_region"
    ATTACHMENT_REGION = "attachment_region"
    IMAGE_REGION = "image_region"


class DocumentLocator(BaseModel):
    locator_kind: Literal["document"] = "document"
    section_ordinal: int = Field(ge=0)
    table_ordinal: Optional[int] = Field(default=None, ge=0)
    row_ordinal: Optional[int] = Field(default=None, ge=0)
    form_ordinal: Optional[int] = Field(default=None, ge=0)
    heading_path: str = ""


class PageLocator(BaseModel):
    locator_kind: Literal["page"] = "page"
    page_ordinal: int = Field(ge=0)
    left: Optional[float] = None
    top: Optional[float] = None
    right: Optional[float] = None
    bottom: Optional[float] = None


class SpreadsheetCell(BaseModel):
    address: str
    row: int = Field(ge=1)
    column: int = Field(ge=1)
    text: str


class SpreadsheetRange(BaseModel):
    a1_range: str
    start_row: int = Field(ge=1)
    start_column: int = Field(ge=1)
    end_row: int = Field(ge=1)
    end_column: int = Field(ge=1)


class SpreadsheetTableIdentity(BaseModel):
    name: str
    display_name: str
    a1_range: str


class SpreadsheetLocator(BaseModel):
    locator_kind: Literal["spreadsheet"] = "spreadsheet"
    sheet_ordinal: int = Field(ge=0)
    sheet_name: str
    region: SpreadsheetRange
    cells: List[SpreadsheetCell] = Field(default_factory=list)
    merged_ranges: List[SpreadsheetRange] = Field(default_factory=list)
    defined_tables: List[SpreadsheetTableIdentity] = Field(default_factory=list)


class ParagraphImageParent(BaseModel):
    model_config = {"extra": "forbid"}

    parent_kind: Literal["paragraph"] = "paragraph"
    section_ordinal: int = Field(ge=0)
    paragraph_ordinal: int = Field(ge=0)


class TableCellImageParent(BaseModel):
    model_config = {"extra": "forbid"}

    parent_kind: Literal["table_cell"] = "table_cell"
    section_ordinal: int = Field(ge=0)
    table_ordinal: int = Field(ge=0)
    row_ordinal: int = Field(ge=0)
    cell_ordinal: int = Field(ge=0)


class FormImageParent(BaseModel):
    model_config = {"extra": "forbid"}

    parent_kind: Literal["form"] = "form"
    section_ordinal: int = Field(ge=0)
    form_ordinal: int = Field(ge=0)


CompoundImageParent = Annotated[
    Union[ParagraphImageParent, TableCellImageParent, FormImageParent],
    Field(discriminator="parent_kind"),
]


class ImageLocator(BaseModel):
    locator_kind: Literal["image"] = "image"
    original_ref: str
    width: int = Field(ge=1)
    height: int = Field(ge=1)
    media_type: str
    page_ordinal: Optional[int] = Field(default=None, ge=0)
    compound_parent: Optional[CompoundImageParent] = None
    left: Optional[float] = None
    top: Optional[float] = None
    right: Optional[float] = None
    bottom: Optional[float] = None

    @model_validator(mode="after")
    def validate_location(self) -> "ImageLocator":
        if self.page_ordinal is not None and self.compound_parent is not None:
            raise ValueError("image cannot have both page and compound parent")
        bounds = (self.left, self.top, self.right, self.bottom)
        if any(value is not None for value in bounds):
            if not all(value is not None for value in bounds):
                raise ValueError("image bounds must be complete")
            if (
                self.left is None
                or self.right is None
                or self.top is None
                or self.bottom is None
            ):
                raise ValueError("image bounds must be complete")
            if self.left >= self.right or self.top >= self.bottom:
                raise ValueError("image bounds must be increasing")
            if self.page_ordinal is None:
                raise ValueError("bounded image requires page_ordinal")
        return self


class AttachmentLocator(BaseModel):
    locator_kind: Literal["attachment"] = "attachment"
    part_name: str
    relationship_type: str


SourceLocator = Union[
    DocumentLocator,
    PageLocator,
    SpreadsheetLocator,
    ImageLocator,
    AttachmentLocator,
]


class StructuredSourceUnit(BaseModel):
    key: str
    ordinal: int = Field(ge=0)
    kind: StructuredSourceUnitKind
    text: str = ""
    locator: SourceLocator


class Document(BaseModel):
    """Document including document content, document metadata."""

    model_config = {"arbitrary_types_allowed": True}

    content: str = Field(default="", description="document text content")
    images: Dict[str, str] = Field(
        default_factory=dict, description="Images in the document"
    )

    chunks: List[Chunk] = Field(default_factory=list, description="document chunks")
    structured_source_units: List[StructuredSourceUnit] = Field(default_factory=list)
    metadata: Dict[str, Any] = Field(
        default_factory=dict,
        description="metadata fields",
    )

    def set_content(self, content: str) -> None:
        """Set document content."""
        self.content = content

    def get_content(self) -> str:
        """Get document content."""
        return self.content

    def is_valid(self) -> bool:
        return self.content != "" or bool(self.structured_source_units)
