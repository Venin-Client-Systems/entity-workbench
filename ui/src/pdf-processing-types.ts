import type { ImageExtraction } from "./image-processing-types";
import type { ProcessingInput } from "./processing-types";

export type PdfRenderLimitation =
  | "scan_focused_subset"
  | "annotations_excluded"
  | "color_converted_to_gray"
  | "unreviewed_raster"
  | "no_word_regions";
export type PdfRenderFailure =
  | "encrypted_document"
  | "unsupported_format"
  | "unsupported_feature"
  | "active_content"
  | "external_resource"
  | "malformed_document"
  | "page_out_of_range"
  | "page_limit"
  | "pixel_limit"
  | "structure_limit"
  | "stream_limit"
  | "operator_limit";

/** Immutable canonical PDF extraction v1; raster bytes are not part of this record. */
export type PdfExtraction = {
  schema_version: 1;
  id: string;
  job_id: string;
  attempt: number;
  input: ProcessingInput & { operation: "pdf_page_ocr" };
  created_at: string;
  result_sha256: string;
  result: {
    raster_retained: false;
    render: {
      protocol_version: 1;
      job_id: string;
      original_sha256: string;
      original_bytes: number;
      renderer: string;
      java_runtime: string;
      page_number: number;
      dpi: number;
      page_count: number | null;
      status:
        "rendered" | "encrypted" | "unsupported" | "failed" | "quota_exhausted";
      failure: PdfRenderFailure | null;
      geometry: {
        crop_box: [number, number, number, number];
        rotation_degrees: number;
        pdf_to_raster: [number, number, number, number, number, number];
      } | null;
      raster: {
        path: "raster.pgm";
        sha256: string;
        bytes: number;
        width: number;
        height: number;
      } | null;
      limitations: PdfRenderLimitation[];
    };
    recognition: ImageExtraction["result"]["recognition"];
  };
};
