import type { ImageExtraction } from "./image-processing-types";
import type { ProcessingInput } from "./processing-types";
export type DerivativeRef = {
  sha256: string;
  bytes: number;
  kind: "canonical_pgm_v1" | "ocr_tsv_v1" | "image_region_result_json_v1";
};
export type OcrRegion = {
  level: "page" | "block" | "paragraph" | "line" | "word";
  page_number: number;
  block_number: number;
  paragraph_number: number;
  line_number: number;
  word_number: number;
  bounds: { left: number; top: number; width: number; height: number };
  engine_confidence: number | null;
  text: string;
};
export type ImageRegionInspection = {
  extraction: {
    schema_version: 1;
    id: string;
    job_id: string;
    attempt: number;
    input: ProcessingInput & { operation: "image_ocr_regions" };
    created_at: string;
    result: DerivativeRef;
    raster: DerivativeRef | null;
    tsv: DerivativeRef | null;
  };
  result: {
    schema_version: 1;
    decoder: ImageExtraction["result"]["decoder"];
    recognition:
      | (Omit<
          NonNullable<ImageExtraction["result"]["recognition"]>,
          "limitations"
        > & {
          tsv_sha256: string;
          tsv_bytes: number;
          regions: OcrRegion[];
          limitations: (
            | "unreviewed_recognition"
            | "unreviewed_word_regions"
            | "engine_confidence_not_probability"
            | "no_original_document_mapping"
            | "single_uniform_block"
          )[];
        })
      | null;
  };
};
