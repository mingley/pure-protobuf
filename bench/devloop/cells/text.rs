//! PK-24 protobuf text-format cells over generated TAT and OTLP messages.

use crate::otlp::{ResourceSpans, ScopeSpans, Span, TracesData};
use pbrs::gencode::TestAllTypesProto3 as Tat;
use pbrs::prelude::*;

pub const CELLS: &[(&str, &str)] = &[
    ("codec.pbrs.text.tat_encode", "pbrs"),
    ("codec.pbrs.text.tat_decode", "pbrs"),
    ("codec.pbrs.text.otlp_encode", "pbrs"),
    ("codec.pbrs.text.otlp_decode", "pbrs"),
];

pub struct TextCase {
    tat: Tat,
    tat_text: String,
    otlp: TracesData,
    otlp_text: String,
}

impl TextCase {
    pub fn prepare(tat: &Tat) -> Self {
        let tat = tat.clone();
        let tat_text = tat.to_text().expect("TAT text fixture");

        let mut span = Span::new();
        span.set_trace_id((0u8..16).collect::<Vec<_>>());
        span.set_span_id((16u8..24).collect::<Vec<_>>());
        span.set_name("GET /v1/widgets");
        span.set_kind(2);
        span.set_start_time_unix_nano(1_725_000_000_000_000_000);
        span.set_end_time_unix_nano(1_725_000_000_000_250_000);
        span.status_mut().set_code(1);
        let mut scope = ScopeSpans::new();
        scope.spans_mut().push(span);
        scope.set_schema_url("https://opentelemetry.io/schemas/1.26.0");
        let mut resource = ResourceSpans::new();
        resource.scope_spans_mut().push(scope);
        let mut otlp = TracesData::new();
        otlp.resource_spans_mut().push(resource);
        let otlp_text = otlp.to_text().expect("OTLP text fixture");

        // Semantic equivalence is checked before timing, not inferred from an
        // output length. This catches fixture/codegen drift immediately.
        assert_eq!(
            Tat::from_text(&tat_text).unwrap().to_text().unwrap(),
            tat_text
        );
        assert_eq!(
            TracesData::from_text(&otlp_text)
                .unwrap()
                .to_text()
                .unwrap(),
            otlp_text
        );
        Self {
            tat,
            tat_text,
            otlp,
            otlp_text,
        }
    }
}

pub fn work(cell: &str, case: &TextCase) -> Option<u64> {
    let value = match cell {
        "codec.pbrs.text.tat_encode" => case.tat.to_text().expect("TAT text encode").len() as u64,
        "codec.pbrs.text.tat_decode" => Tat::from_text(&case.tat_text)
            .expect("TAT text decode")
            .serialize()
            .expect("TAT wire checksum")
            .len() as u64,
        "codec.pbrs.text.otlp_encode" => {
            case.otlp.to_text().expect("OTLP text encode").len() as u64
        }
        "codec.pbrs.text.otlp_decode" => TracesData::from_text(&case.otlp_text)
            .expect("OTLP text decode")
            .serialize()
            .expect("OTLP wire checksum")
            .len() as u64,
        _ => return None,
    };
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_cells_execute_against_round_tripping_fixtures() {
        let mut tat = Tat::new();
        tat.set_optional_string("text fixture");
        let case = TextCase::prepare(&tat);
        for (cell, _) in CELLS {
            assert!(work(cell, &case).unwrap() > 0, "{cell}");
        }
    }
}
