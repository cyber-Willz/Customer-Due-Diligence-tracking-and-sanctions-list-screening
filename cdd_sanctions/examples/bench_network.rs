//! Scaling check for the spectral network analysis (synthetic data):
//! `cargo run --release --example bench_network [PARTIES] [--links]`
//! `--links` also gives every party a unique phone and address, ~4% a shared bank account (groups of ~2),
//! and 10% of entities one of 20 busy registered agents (groups far above the cap, so skipped).

use cdd_sanctions::cdd::*;
use cdd_sanctions::hypernet::{self, NetworkConfig};
use cdd_sanctions::links::{Link, LinkKind};
use std::collections::BTreeMap;
use std::time::Instant;

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

fn main() {
    let n: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(20_000);
    let with_links = std::env::args().any(|a| a == "--links");
    let mut rng = Lcg(42);
    let mut parties: BTreeMap<PartyId, Party> = BTreeMap::new();
    let (entities, people) = (n / 3, n - n / 3);
    for id in 1..=n {
        let kind = if id <= entities { PartyKind::Entity } else { PartyKind::Individual };
        let mut p = Party::new(kind, &format!("P{id}"));
        p.id = id;
        parties.insert(id, p);
    }
    // every entity: 1-3 owners and 1-3 directors drawn from the people pool; directors are
    // reused across companies (skewed towards low ids) so components grow
    let mut rels = Vec::new();
    let mut rid = 0;
    let person = |rng: &mut Lcg| -> PartyId {
        let r = rng.next() % 1000;
        let span = if r < 700 { people / 10 } else { people };
        entities + 1 + rng.next() % span.max(1)
    };
    for e in 1..=entities {
        for _ in 0..(1 + rng.next() % 3) {
            rid += 1;
            let owner = person(&mut rng);
            rels.push(Relationship { id: rid, from: owner, to: e, kind: RelKind::Owner, percent: Some(20.0 + (rng.next() % 60) as f64) });
        }
        for _ in 0..(1 + rng.next() % 3) {
            rid += 1;
            let d = person(&mut rng);
            rels.push(Relationship { id: rid, from: d, to: e, kind: RelKind::Director, percent: None });
        }
    }
    if with_links {
        let pool = (n / 40).max(1);
        for id in 1..=n {
            let r1 = rng.next() % 100;
            let r2 = rng.next() % pool;
            let r3 = rng.next() % 20;
            let p = parties.get_mut(&id).unwrap();
            p.links.push(Link::new(LinkKind::Phone, &format!("+501 6{:06}", id), None));
            p.links.push(Link::new(LinkKind::Address, &format!("{id} Main Street, Belize City"), Some("BZ")));
            if r1 < 4 {
                p.links.push(Link::new(LinkKind::BankAccount, &format!("00{:08}", r2), Some("BZ")));
            }
            if id <= entities && r1 >= 90 {
                p.links.push(Link::new(LinkKind::RegisteredAgent, &format!("Busy Agent {r3} Ltd"), Some("BZ")));
            }
        }
    }
    let cfg = NetworkConfig::default();
    println!("{} parties, {} relationships{}", parties.len(), rels.len(), if with_links { " (+ links)" } else { "" });

    let t = Instant::now();
    let (specs, skipped) = hypernet::edge_specs(&parties, &rels, &cfg);
    println!("edge_specs:          {:>8.1} ms ({} hyperedges, {} groups skipped as too large)", t.elapsed().as_secs_f64() * 1e3, specs.len(), skipped);
    let t = Instant::now();
    let comps = hypernet::components(specs, None).unwrap();
    let biggest = comps.iter().map(|c| c.len()).max().unwrap_or(0);
    println!("components:          {:>8.1} ms ({} components, largest {} parties)", t.elapsed().as_secs_f64() * 1e3, comps.len(), biggest);

    let seeds: Vec<(PartyId, f64)> = (0..20).map(|i| (entities + 1 + i * (people / 25), 1.0)).collect();
    let t = Instant::now();
    let big = comps.iter().max_by_key(|c| c.len()).unwrap();
    let local: Vec<(PartyId, f64)> = seeds.iter().copied().filter(|(s, _)| big.contains(*s)).collect();
    let ex = big.exposure_all(&local, &cfg).unwrap();
    println!("exposure_all:        {:>8.1} ms ({} seeds in the largest component, {} exposed)", t.elapsed().as_secs_f64() * 1e3, local.len(), ex.iter().filter(|x| x.0 > 0.0).count());

    let root = big.parties()[big.len() / 2];
    let t = Instant::now();
    let r = hypernet::exposure_for(&parties, &rels, &local, root, &cfg).unwrap();
    println!("exposure_for (1 party, incl. edge build): {:>6.1} ms (exposure {:.5})", t.elapsed().as_secs_f64() * 1e3, r.exposure);

    let t = Instant::now();
    let s = big.structure(&cfg);
    println!("structure (largest): {:>8.1} ms (lambda2 {:?}, split {}, note {:?})", t.elapsed().as_secs_f64() * 1e3, s.algebraic_connectivity, s.split.is_some(), s.note);

    let t = Instant::now();
    let rep = hypernet::analyze(&parties, &rels, &seeds, &cfg).unwrap();
    println!("analyze (everything): {:>7.1} ms ({} reported components, {} exposures)", t.elapsed().as_secs_f64() * 1e3, rep.components.len(), rep.exposures.len());
}
