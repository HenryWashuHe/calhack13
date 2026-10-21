/** JSON contract shared with genesis-verification's Rust model. */
export interface AvailabilityWindow { start: string; end: string }
export interface ActorInput {
  source_url: string;
  attendees: number;
  requested_window: AvailabilityWindow;
}
export interface EvidenceRef {
  source_url: string;
  snapshot_sha256: string;
  locator: string;
}
export interface Venue {
  venue_id: string;
  capacity: number;
  price: { minor_units: number; currency: string };
  window: AvailabilityWindow;
  availability: 'available' | 'unavailable';
  step_free: boolean;
  evidence: EvidenceRef;
}
export type ExtractionOutcome =
  | { status: 'supported'; venues: Venue[] }
  | { status: 'unsupported'; reason: 'missing_required_data' | 'unsupported_format'; evidence: EvidenceRef };
export interface ExtractionContext {
  readonly source_url: string;
  readonly snapshot_sha256: string;
  readonly input: Readonly<Omit<ActorInput, 'requested_window'>> & {
    readonly requested_window: Readonly<AvailabilityWindow>;
  };
}
export type Extractor = (html: string, context: ExtractionContext) => ExtractionOutcome | Promise<ExtractionOutcome>;
