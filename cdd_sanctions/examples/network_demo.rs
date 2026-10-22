//! Spectral network analysis demo: `cargo run --release --example network_demo`
//!
//! A sanctioned individual (confirmed by the MLRO) owns 80% of "Volga Trading". The question
//! the ownership rules cannot answer: which *other* customers are connected to that structure
//! through shared directors or a reused passport number, and how closely?

use cdd_sanctions::cdd::*;
use cdd_sanctions::engine::*;
use cdd_sanctions::model::*;
use chrono::{TimeZone, Utc};

fn main() {
    let now = Utc.with_ymd_and_hms(2026, 10, 13, 9, 0, 0).unwrap();
    let (ana, mia) = (Actor::new("ana", Role::Analyst), Actor::new("mia", Role::Mlro));
    let mut c = Compliance::default();
    if let Some(a) = std::env::var("DEMO_DAMPING").ok().and_then(|v| v.parse().ok()) {
        c.risk_cfg.network.damping = a;
    }

    let listed = ListEntry {
        list: "DEMO".into(),
        uid: "1".into(),
        kind: SubjectKind::Individual,
        names: vec![NameVariant { name: "PETROV, Ivan Sergeyevich".into(), strength: AliasStrength::Primary }],
        dobs: vec![PartialDate::ymd(1965, 3, 12)],
        nationalities: vec!["RU".into()],
        countries: vec![],
        identifiers: vec![],
        programs: vec!["DEMO-PROGRAM".into()],
        remarks: None,
        listed_on: None,
        content_hash: String::new(),
    }
    .finalize();
    c.load_list("DEMO", vec![listed], "v1", false, &mia, now).unwrap();

    let add = |c: &mut Compliance, kind: PartyKind, name: &str| -> PartyId {
        let mut p = Party::new(kind, name);
        p.nationalities = vec!["Belize".into()];
        c.add_party(p, &ana, now).unwrap()
    };
    let ind = PartyKind::Individual;
    let ent = PartyKind::Entity;

    // the sanctioned structure
    let petrov = {
        let mut p = Party::new(ind, "Ivan Petrov");
        p.dob = vec![PartialDate::ymd(1965, 3, 12)];
        p.nationalities = vec!["Russia".into()];
        c.add_party(p, &ana, now).unwrap()
    };
    let volga = add(&mut c, ent, "Volga Trading Ltd");
    let orlov = add(&mut c, ind, "Dmitri Orlov");
    c.add_relationship(petrov, volga, RelKind::Owner, Some(80.0), &ana, now).unwrap();
    c.add_relationship(orlov, volga, RelKind::Director, None, &ana, now).unwrap();

    // customers at varying distance
    let direct = add(&mut c, ent, "Shared Director Ltd"); // same director as Volga
    c.add_relationship(orlov, direct, RelKind::Director, None, &ana, now).unwrap();

    let (e12, e13) = (add(&mut c, ent, "Chain Customer Ltd"), add(&mut c, ent, "Mid Co Ltd"));
    let d12 = add(&mut c, ind, "Elena Ruiz");
    c.add_relationship(d12, e12, RelKind::Director, None, &ana, now).unwrap();
    c.add_relationship(d12, e13, RelKind::Director, None, &ana, now).unwrap();
    c.add_relationship(orlov, e13, RelKind::Director, None, &ana, now).unwrap();

    let hub = add(&mut c, ent, "Big Holdings plc");
    c.add_relationship(petrov, hub, RelKind::Owner, Some(4.0), &ana, now).unwrap();
    let neighbour_owner = add(&mut c, ind, "Hub Co-owner");
    c.add_relationship(neighbour_owner, hub, RelKind::Owner, Some(4.0), &ana, now).unwrap();
    for i in 0..23 {
        let o = add(&mut c, ind, &format!("Hub Shareholder {i}"));
        c.add_relationship(o, hub, RelKind::Owner, Some(4.0), &ana, now).unwrap();
    }
    let hub_neighbour = add(&mut c, ent, "Hub Neighbour Ltd");
    c.add_relationship(neighbour_owner, hub_neighbour, RelKind::Director, None, &ana, now).unwrap();

    let twin_a = add(&mut c, ind, "Passport Twin A"); // director at Volga
    c.add_relationship(twin_a, volga, RelKind::Director, None, &ana, now).unwrap();
    let twin_b = add(&mut c, ind, "Passport Twin B"); // new customer quoting the same passport
    for t in [twin_a, twin_b] {
        c.update_party(t, |p| p.identifiers.push(Identifier::new(IdKind::Passport, "P9988776", Some("BZ"))), &ana, now).unwrap();
    }

    let quiet = add(&mut c, ent, "Quiet Customer Ltd");
    let quiet_dir = add(&mut c, ind, "Unrelated Director");
    c.add_relationship(quiet_dir, quiet, RelKind::Director, None, &ana, now).unwrap();

    // screen Petrov, MLRO confirms
    c.screen_party(petrov, &ana, now).unwrap();
    let alert = c.alerts().values().next().expect("alert").id;
    c.resolve_alert(alert, Outcome::ConfirmedMatch, "demo: passport and DOB match listing", &mia, now).unwrap();

    println!("== per-customer indirect exposure (sanctioned seed: Ivan Petrov) ==");
    for (label, id) in [
        ("shares a director with Volga", direct),
        ("two directors removed", e12),
        ("one of 25 co-owners' directorship", hub_neighbour),
        ("same passport as Volga's director", twin_b),
        ("no connection", quiet),
    ] {
        let e = c.network_exposure(id).unwrap();
        let a = c.assess_risk(id, now).unwrap();
        println!(
            "{:<38} exposure {:.4}  hops {:?}  -> {:?} (score {}, DD {:?})",
            label, e.exposure, e.hops, a.tier, a.score, a.dd_level
        );
    }

    let rep = c.network_analysis().unwrap();
    println!("\n== portfolio analysis: {} components ==", rep.components.len());
    for comp in &rep.components {
        let s = &comp.structure;
        println!(
            "component {:>3}: {:>2} parties, {:>2} hyperedges, seeds {:?}, lambda2 {:?}{}",
            comp.id,
            comp.parties.len(),
            comp.hyperedges,
            comp.seeds,
            s.algebraic_connectivity.map(|x| (x * 1000.0).round() / 1000.0),
            s.split.as_ref().map(|sp| format!(", weakest cut conductance {:.3}, connectors {:?}", sp.conductance, sp.connectors)).unwrap_or_default()
        );
    }
    println!("\ntop exposures:");
    for e in rep.exposures.iter().take(6) {
        let name = c.party(e.party).map(|p| p.name.as_str()).unwrap_or("?");
        println!("  {:.4}  {:<24} hops {:?}  covered_by_ownership_rules={}", e.exposure, name, e.hops, e.covered_by_ownership_rules);
    }
    println!("\nwhy is '{}' exposed?", c.party(twin_b).unwrap().name);
    for line in c.network_exposure(twin_b).unwrap().path {
        println!("  {line}");
    }
}
