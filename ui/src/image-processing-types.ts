import type { ProcessingInput } from "./processing-types";

export type DecodeLimitation =
  | "exif_orientation_not_applied"
  | "embedded_previews_excluded"
  | "metadata_not_extracted"
  | "color_converted_to_gray"
  | "no_document_page_mapping";
export type RecognitionLimitation =
  "unreviewed_recognition" | "no_word_regions" | "no_original_document_mapping";
export type ImageExtraction = {
  schema_version: 1;
  id: string;
  job_id: string;
  attempt: number;
  input: ProcessingInput & { operation: "image_ocr" };
  created_at: string;
  result_sha256: string;
  result: {
    raster_retained: false;
    decoder: {
      protocol_version: 1;
      job_id: string;
      original_sha256: string;
      original_bytes: number;
      decoder: string;
      java_runtime: string;
      media_type: string;
      status: "decoded" | "unsupported" | "failed" | "quota_exhausted";
      failure:
        | "unsupported_format"
        | "multiple_images"
        | "malformed_image"
        | "decoder_warning"
        | "pixel_limit"
        | "container_limit"
        | null;
      raster: {
        path: "raster.pgm";
        sha256: string;
        bytes: number;
        width: number;
        height: number;
        source_image_index: 0;
        pixel_mapping: "encoded_pixels_gray_white_alpha_v1";
      } | null;
      limitations: DecodeLimitation[];
    };
    recognition: {
      protocol_version: 1;
      job_id: string;
      raster_sha256: string;
      raster_bytes: number;
      width: number;
      height: number;
      engine: string;
      language: "eng";
      model_sha256: string;
      runtime_manifest_sha256: string;
      status: "recognized" | "no_text_recognized";
      text: string;
      limitations: RecognitionLimitation[];
    } | null;
  };
};
