//! Synthetic end-to-end check of the learned alert prioritiser:
//! `cargo run --release --example ml_demo [CUSTOMERS] [SEED]`
//!
//! What this is: a *machinery* test. A synthetic list and customer base are generated with a known ground truth
//! (a true match is a perturbed copy of a list entry; everything else is drawn from the same name pools, so common
//! names collide often), every customer is screened by the real engine, and the real trainer is run on the
//! resulting hits. The numbers show the trainer, the cross-validation, the gates and a held-out check work.
//! They say nothing about how much it will help on YOUR data: that needs your own analyst dispositions.

use cdd_sanctions::ml::{self, LogisticModel, TrainOptions};
use cdd_sanctions::model::*;
use cdd_sanctions::screening::*;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn zipf(&mut self, n: usize) -> usize {
        // P(i) ~ 1/(i+1)
        let h: f64 = (1..=n).map(|i| 1.0 / i as f64).sum();
        let mut u = self.f() * h;
        for i in 0..n {
            u -= 1.0 / (i + 1) as f64;
            if u <= 0.0 {
                return i;
            }
        }
        n - 1
    }
}

const SYL: [&str; 20] = ["ka", "lo", "mi", "ra", "su", "te", "vo", "ni", "da", "be", "zu", "fa", "go", "hi", "je", "pu", "an", "or", "el", "us"];
const NATS: [&str; 12] = ["RU", "IR", "SY", "KP", "VE", "BY", "MM", "CU", "SD", "LY", "YE", "AF"];

fn word(r: &mut Rng) -> String {
    let n = 2 + r.below(3);
    (0..n).map(|_| SYL[r.below(20)]).collect()
}

struct World {
    given: Vec<String>,
    common_sur: Vec<String>,
}

#[derive(Clone)]
struct Person {
    first: String,
    middle: Option<String>,
    last: String,
    dob: Option<PartialDate>,
    nat: Option<&'static str>,
}

fn person(w: &World, r: &mut Rng) -> Person {
    let last = if r.f() < 0.40 { w.common_sur[r.zipf(w.common_sur.len())].clone() } else { word(r) + &word(r) };
    Person {
        first: w.given[r.zipf(w.given.len())].clone(),
        middle: if r.f() < 0.5 { Some(w.given[r.zipf(w.given.len())].clone()) } else { None },
        last,
        dob: Some(PartialDate::ymd(1945 + r.below(55) as i32, 1 + r.below(12) as u8, 1 + r.below(28) as u8)),
        nat: Some(NATS[r.below(NATS.len())]),
    }
}

fn typo(s: &str, r: &mut Rng) -> String {
    let mut c: Vec<char> = s.chars().collect();
    if c.len() > 3 {
        let i = 1 + r.below(c.len() - 2);
        c.swap(i, i + 1);
    }
    c.into_iter().collect()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n_cust: usize = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(8000);
    let seed: u64 = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(11);
    let mut r = Rng(seed);
    let w = World { given: (0..400).map(|_| word(&mut r) + &word(&mut r)).collect(), common_sur: (0..300).map(|_| word(&mut r) + &word(&mut r)).collect() };

    // list of 5,000 individuals
    let mut listed: Vec<Person> = Vec::new();
    for _ in 0..5000 {
        let mut p = person(&w, &mut r);
        let u = r.f();
        if u > 0.7 {
            p.dob = p.dob.map(|d| PartialDate::year(d.year.unwrap()));
        }
        if u > 0.9 {
            p.dob = None;
        }
        if r.f() > 0.8 {
            p.nat = None;
        }
        listed.push(p);
    }
    let entries: Vec<ListEntry> = listed
        .iter()
        .enumerate()
        .map(|(i, p)| ListEntry {
            list: "SYN".into(),
            uid: i.to_string(),
            kind: SubjectKind::Individual,
            names: vec![NameVariant { name: format!("{}, {}{}", p.last.to_uppercase(), p.first, p.middle.as_ref().map(|m| format!(" {m}")).unwrap_or_default()), strength: AliasStrength::Primary }],
            dobs: p.dob.into_iter().collect(),
            nationalities: p.nat.map(|n| vec![n.to_string()]).unwrap_or_default(),
            countries: vec![],
            identifiers: vec![],
            programs: vec!["SYN".into()],
            remarks: None,
            listed_on: None,
            content_hash: String::new(),
        }.finalize())
        .collect();
    let idx = SanctionsIndex::build(entries);
    let cfg = ScreeningConfig::default();

    // customers: 3% are really a listed person (perturbed), the rest are unrelated people from the same name pools
    let mut examples_by_time: Vec<Vec<ml::Example>> = Vec::new();
    let (mut true_customers, mut missed_by_rules) = (0, 0);
    for cust in 0..n_cust {
        let is_match = r.f() < 0.03;
        let (subject, origin) = if is_match {
            let oi = r.below(listed.len());
            let o = &listed[oi];
            let mut first = o.first.clone();
            let mut last = o.last.clone();
            if r.f() < 0.25 {
                last = typo(&last, &mut r);
            }
            if r.f() < 0.15 {
                first = typo(&first, &mut r);
            }
            let name = match (r.f(), &o.middle) {
                (x, Some(m)) if x < 0.7 => format!("{first} {m} {last}"),
                _ => format!("{first} {last}"),
            };
            let mut s = Subject::new(SubjectKind::Individual, &name);
            if r.f() < 0.8 {
                s.dob = match o.dob {
                    Some(d) if r.f() < 0.9 => vec![d],
                    Some(d) => vec![PartialDate::ymd(d.year.unwrap(), d.month.unwrap_or(1), 1 + r.below(28) as u8)], // data-entry error
                    None => vec![],
                };
            }
            if r.f() < 0.7 {
                s.nationalities = o.nat.map(|n| vec![n.to_string()]).unwrap_or_default();
            }
            (s, Some(oi))
        } else {
            let p = person(&w, &mut r);
            let mut s = Subject::new(SubjectKind::Individual, &format!("{} {}", p.first, p.last));
            if r.f() < 0.6 {
                s.dob = p.dob.into_iter().collect();
            }
            if r.f() < 0.6 {
                s.nationalities = p.nat.map(|n| vec![n.to_string()]).unwrap_or_default();
            }
            (s, None)
        };
        let res = idx.screen(&subject, &cfg);
        let mut exs = Vec::new();
        for h in &res.hits {
            let y = origin.map_or(false, |o| h.uid == o.to_string());
            exs.push(ml::Example::new(&h.features, y, cust as u64));
        }
        if is_match {
            true_customers += 1;
            if !exs.iter().any(|e| e.y) {
                missed_by_rules += 1;
            }
        }
        examples_by_time.push(exs);
    }

    // time split: train/CV on the first 70% of customers, hold the last 30% out entirely
    let cut = n_cust * 7 / 10;
    let train_ex: Vec<ml::Example> = examples_by_time[..cut].iter().flatten().cloned().collect();
    let hold_ex: Vec<ml::Example> = examples_by_time[cut..].iter().flatten().cloned().collect();
    println!(
        "{n_cust} customers, {true_customers} truly listed; the rules missed {missed_by_rules} of them (no alert raised: no ML can recover these)"
    );
    println!("alerts: {} total; train/CV {} ({} true), hold-out {} ({} true)", train_ex.len() + hold_ex.len(), train_ex.len(), train_ex.iter().filter(|e| e.y).count(), hold_ex.len(), hold_ex.iter().filter(|e| e.y).count());

    let rep = ml::train(&train_ex, &TrainOptions { label: "synthetic".into(), ..Default::default() }).expect("train");
    let pct = |a: Option<f64>| a.map(|v| format!("{:.1}%", v * 100.0)).unwrap_or("n/a".into());
    let f3 = |a: Option<f64>| a.map(|v| format!("{v:.3}")).unwrap_or("n/a".into());
    println!("\nout-of-fold on the training period ({}-fold, grouped by customer, ridge {}):", rep.folds, rep.lambda);
    println!("{:<22}{:>8}{:>8}{:>12}{:>12}", "", "AUC", "AP", "queue@95%", "queue@100%");
    for (n, m) in [("learned model", &rep.model), ("hand-set scorecard", &rep.prior), ("current rule score", &rep.rule)] {
        println!("{:<22}{:>8}{:>8}{:>12}{:>12}", n, f3(m.auc), f3(m.average_precision), pct(m.queue_at_95_recall), pct(m.queue_at_100_recall));
    }
    if let Some((lo, hi)) = rep.auc_gain_vs_rule {
        println!("AUC gain over rule score, 95% bootstrap interval: [{lo:+.3}, {hi:+.3}]");
    }
    for g in &rep.gates {
        println!("gate [{}] {} - {}", if g.passed { "pass" } else { "FAIL" }, g.name, g.detail);
    }
    for wn in &rep.warnings {
        println!("warning: {wn}");
    }

    // held-out period (never seen by training, CV or lambda selection)
    let eval = |name: &str, f: &dyn Fn(&ml::Example) -> f64| {
        let s: Vec<(bool, f64)> = hold_ex.iter().map(|e| (e.y, f(e))).collect();
        println!("{:<22}{:>8}{:>8}{:>12}{:>12}", name, f3(cdd_sanctions::calibrate::auc(&s)), f3(ml::average_precision(&s)), pct(ml::queue_fraction_at_recall(&s, 0.95)), pct(ml::queue_fraction_at_recall(&s, 1.0)));
    };
    let prior = LogisticModel::prior();
    println!("\nheld-out later period (never used for fitting or selection):");
    println!("{:<22}{:>8}{:>8}{:>12}{:>12}", "", "AUC", "AP", "queue@95%", "queue@100%");
    eval("learned model", &|e| rep.candidate.predict_vec(&e.x));
    eval("hand-set scorecard", &|e| prior.predict_vec(&e.x));
    eval("current rule score", &|e| e.rule_score);
    println!("\nlargest learned weights (learned / scorecard):");
    for (n, wt, p) in &rep.top_weights {
        println!("  {n:<22}{wt:>8.2}{p:>8.2}");
    }
}
