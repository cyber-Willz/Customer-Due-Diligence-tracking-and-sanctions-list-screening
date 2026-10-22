//! Synthetic scale check: `cargo run --release --example bench`
use cdd_sanctions::model::*;
use cdd_sanctions::screening::*;
use std::time::Instant;

fn main() {
    let syl = ["ka", "lo", "mi", "ra", "su", "te", "vo", "ni", "da", "be", "zu", "fa", "go", "hi", "je", "pu", "an", "or", "el", "us"];
    let name = |i: usize, k: usize| format!("{}{}{}{}", syl[(i >> k) % 20], syl[(i / 7 + k) % 20], syl[(i / 131 + 3 * k) % 20], syl[(i / 977 + k) % 20]);
    let n = 100_000;
    let t = Instant::now();
    let entries: Vec<ListEntry> = (0..n)
        .map(|i| ListEntry {
            list: "BENCH".into(), uid: i.to_string(), kind: SubjectKind::Individual,
            names: vec![
                NameVariant { name: format!("{} {}", name(i, 1), name(i, 4)), strength: AliasStrength::Primary },
                NameVariant { name: format!("{} {}", name(i, 2), name(i, 5)), strength: AliasStrength::Strong },
            ],
            dobs: vec![PartialDate::year(1940 + (i % 60) as i32)], nationalities: vec![], countries: vec![],
            identifiers: vec![], programs: vec![], remarks: None, listed_on: None, content_hash: String::new(),
        }.finalize())
        .collect();
    println!("generated {n} entries in {:?}", t.elapsed());
    let t = Instant::now();
    let idx = SanctionsIndex::build(entries.clone());
    println!("index built in {:?}", t.elapsed());

    let cfg = ScreeningConfig::default();
    let queries: Vec<Subject> = (0..500).map(|q| {
        let i = q * 191 % n;
        let mut s = Subject::new(SubjectKind::Individual, &format!("{} {}", name(i, 4), name(i, 1))); // reversed order
        if q % 2 == 0 { s.dob = vec![PartialDate::year(1940 + (i % 60) as i32)]; }
        s
    }).collect();
    let t = Instant::now();
    let (mut found, mut cands) = (0, 0);
    for (q, s) in queries.iter().enumerate() {
        let r = idx.screen(s, &cfg);
        cands += r.candidates_evaluated;
        let i = q * 191 % n;
        if r.hits.iter().any(|h| h.uid == i.to_string()) { found += 1; }
    }
    let el = t.elapsed();
    println!("{} screens in {:?} ({:?}/screen), avg candidates {}, expected entry found in {}/{}", queries.len(), el, el / queries.len() as u32, cands / queries.len(), found, queries.len());
    let t = Instant::now();
    let r = idx.screen(&Subject::new(SubjectKind::Individual, "Zzyzx Qwertyuiop"), &cfg);
    println!("no-match query: {:?}, hits {}", t.elapsed(), r.hits.len());
}
