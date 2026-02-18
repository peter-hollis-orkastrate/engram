//! Benchmark tests for PII detection overhead (DoD NF-3).
//!
//! # Non-Functional Requirement
//!
//! - **NF-3**: Phone PII detection overhead < 1ms per chunk.
//!
//! This benchmark measures the time for `SafetyGate::check` to process
//! realistic text chunks containing phone numbers in various formats.
//! The safety gate runs all enabled PII detectors (email, SSN, credit card,
//! phone), so the benchmark captures the full pipeline cost.

use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion};
use engram_core::config::SafetyConfig;
use engram_core::safety::SafetyGate;

/// Generate a realistic text chunk (~100 words) containing a phone number.
///
/// The phone number format varies by index to exercise all detection paths.
fn generate_chunk_with_phone(index: usize) -> String {
    let phone = match index % 6 {
        0 => "(555) 123-4567".to_string(),
        1 => "555-987-6543".to_string(),
        2 => "+15551234567".to_string(),
        3 => "+1 800 555 1234".to_string(),
        4 => "(800)555-1234".to_string(),
        _ => "555.123.4567".to_string(),
    };

    format!(
        "Meeting notes from the product review session on January fifteenth. \
         The team discussed the upcoming release timeline and identified three \
         critical blockers that need resolution before the launch date. Sarah \
         mentioned that the customer feedback survey results are now available \
         and the satisfaction score improved by four percentage points compared \
         to last quarter. For follow-up questions, contact the project lead at \
         {} during business hours. The next sync is scheduled for Thursday \
         afternoon in conference room B. Action items include updating the \
         deployment runbook and finalizing the monitoring alert thresholds. \
         Chunk reference number {}.",
        phone, index
    )
}

/// Generate a realistic text chunk without any PII (baseline).
fn generate_clean_chunk(index: usize) -> String {
    format!(
        "Meeting notes from the product review session on January fifteenth. \
         The team discussed the upcoming release timeline and identified three \
         critical blockers that need resolution before the launch date. Sarah \
         mentioned that the customer feedback survey results are now available \
         and the satisfaction score improved by four percentage points compared \
         to last quarter. For follow-up questions, contact the project lead \
         during regular business hours at the main office. The next sync is \
         scheduled for Thursday afternoon in conference room B. Action items \
         include updating the deployment runbook and finalizing the monitoring \
         alert thresholds for the production environment. Reference {}.",
        index
    )
}

/// Benchmark SafetyGate::check with phone numbers present.
///
/// DoD NF-3: < 1ms per chunk.
fn bench_pii_phone_detection(c: &mut Criterion) {
    let gate = SafetyGate::new(SafetyConfig::default());

    // Pre-generate chunks to exclude generation time from measurements.
    let chunks_with_phone: Vec<String> = (0..1000).map(generate_chunk_with_phone).collect();
    let clean_chunks: Vec<String> = (0..1000).map(generate_clean_chunk).collect();

    let mut group = c.benchmark_group("pii_detection");
    group.sample_size(200);
    group.measurement_time(Duration::from_secs(10));

    // Benchmark: single chunk with phone number
    group.bench_function("phone_single_chunk", |b| {
        let mut idx = 0usize;
        b.iter(|| {
            let chunk = &chunks_with_phone[idx % chunks_with_phone.len()];
            let decision = gate.check(chunk);
            idx += 1;
            decision
        });
    });

    // Benchmark: single clean chunk (baseline for overhead comparison)
    group.bench_function("clean_single_chunk", |b| {
        let mut idx = 0usize;
        b.iter(|| {
            let chunk = &clean_chunks[idx % clean_chunks.len()];
            let decision = gate.check(chunk);
            idx += 1;
            decision
        });
    });

    // Benchmark: batch of 100 chunks with phones
    group.bench_function("phone_batch_100", |b| {
        b.iter(|| {
            let mut decisions = Vec::with_capacity(100);
            for chunk in &chunks_with_phone[..100] {
                decisions.push(gate.check(chunk));
            }
            decisions
        });
    });

    group.finish();
}

/// Explicit p95 latency assertion for NF-3.
///
/// Measures 1000 individual check calls and asserts the 95th percentile
/// is under the 1ms target.
fn bench_pii_latency_assertion(c: &mut Criterion) {
    let gate = SafetyGate::new(SafetyConfig::default());
    let chunks: Vec<String> = (0..1000).map(generate_chunk_with_phone).collect();

    let target = Duration::from_micros(1000); // 1ms = 1000us

    let mut group = c.benchmark_group("pii_latency_assertion");
    group.sample_size(200);
    group.measurement_time(Duration::from_secs(5));

    group.bench_function("phone_pii_per_chunk", |b| {
        let mut idx = 0usize;
        b.iter(|| {
            let chunk = &chunks[idx % chunks.len()];
            let decision = gate.check(chunk);
            idx += 1;
            decision
        });
    });

    group.finish();

    // Standalone p95 measurement with explicit assertion.
    let mut times = Vec::with_capacity(1000);
    for chunk in &chunks {
        let start = std::time::Instant::now();
        let _decision = gate.check(chunk);
        times.push(start.elapsed());
    }

    times.sort();
    let p95 = times[949]; // 95th percentile of 1000 samples
    let p99 = times[989]; // 99th percentile
    let median = times[499];
    let max = *times.last().unwrap();

    eprintln!("\n=== PII Detection Latency (1000 chunks with phone numbers) ===");
    eprintln!("Median:  {:?}", median);
    eprintln!("p95:     {:?} (target: {:?})", p95, target);
    eprintln!("p99:     {:?}", p99);
    eprintln!("Max:     {:?}", max);

    assert!(
        p95 < target,
        "NF-3 FAILED: Phone PII detection p95 {:?} exceeds target {:?}",
        p95,
        target
    );

    eprintln!("NF-3: PASS (phone PII p95 {:?} < {:?})", p95, target);
}

criterion_group!(
    benches,
    bench_pii_phone_detection,
    bench_pii_latency_assertion
);
criterion_main!(benches);
