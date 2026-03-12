//! Test result tracking and report generation.

use std::fmt::Write as _;

/// Single test outcome.
#[derive(Debug, Clone)]
struct Entry {
    id: String,
    scenario: String,
    category: String,
    desc: String,
    passed: bool,
    detail: String,
}

/// Accumulates test results across all scenarios.
pub struct Results {
    entries: Vec<Entry>,
    scenario: String,
    category: String,
}

impl Results {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            scenario: String::new(),
            category: String::new(),
        }
    }

    /// Set the current scenario label.
    pub fn set_scenario(&mut self, name: &str) {
        self.scenario = name.to_string();
    }

    /// Set the current test category label.
    pub fn set_category(&mut self, name: &str) {
        self.category = name.to_string();
    }

    /// Record a test result and print a live status line.
    pub fn record(&mut self, id: &str, desc: &str, passed: bool, detail: &str) {
        let icon = if passed { "PASS" } else { "FAIL" };
        let emoji = if passed { '\u{2705}' } else { '\u{274C}' };
        let mut line = format!("  {emoji} [{icon}] {id}: {desc}");
        if !detail.is_empty() {
            let _ = write!(line, "  ({detail})");
        }
        println!("{line}");

        self.entries.push(Entry {
            id: id.into(),
            scenario: self.scenario.clone(),
            category: self.category.clone(),
            desc: desc.into(),
            passed,
            detail: detail.into(),
        });
    }

    /// (total, passed, failed) across all scenarios.
    pub fn totals(&self) -> (usize, usize, usize) {
        let t = self.entries.len();
        let p = self.entries.iter().filter(|e| e.passed).count();
        (t, p, t - p)
    }

    /// (total, passed, failed) for one scenario.
    pub fn scenario_totals(&self, name: &str) -> (usize, usize, usize) {
        let scn: Vec<_> = self
            .entries
            .iter()
            .filter(|e| e.scenario == name)
            .collect();
        let t = scn.len();
        let p = scn.iter().filter(|e| e.passed).count();
        (t, p, t - p)
    }

    /// Build the final paste-friendly report.
    pub fn report(&self) -> String {
        let (total, passed, failed) = self.totals();
        let mut out = String::new();
        let _ = writeln!(out);
        let _ = writeln!(out, "{}", "=".repeat(70));
        let _ = writeln!(out, "  HARDWARE VALIDATION REPORT  (sequent-gateway validate)");
        let _ = writeln!(out, "  Result: {passed}/{total} passed, {failed} failed");
        let _ = writeln!(out, "{}", "=".repeat(70));

        let mut cur_scn = "";
        let mut cur_cat = "";
        for e in &self.entries {
            if e.scenario != cur_scn {
                cur_scn = &e.scenario;
                let (st, sp, sf) = self.scenario_totals(cur_scn);
                let _ = writeln!(out);
                let _ = writeln!(
                    out,
                    "  >> Scenario: {cur_scn}  ({sp}/{st}, {sf} failed)"
                );
                cur_cat = "";
            }
            if e.category != cur_cat {
                cur_cat = &e.category;
                let _ = writeln!(out, "    --- {cur_cat} ---");
            }
            let tag = if e.passed { "PASS" } else { "FAIL" };
            let _ = write!(out, "    [{tag}] {}: {}", e.id, e.desc);
            if !e.detail.is_empty() {
                let _ = write!(out, "  ({})", e.detail);
            }
            let _ = writeln!(out);
        }

        let _ = writeln!(out);
        let _ = writeln!(out, "  TOTAL: {passed}/{total} passed");
        if failed > 0 {
            let ids: Vec<&str> = self
                .entries
                .iter()
                .filter(|e| !e.passed)
                .map(|e| e.id.as_str())
                .collect();
            let _ = writeln!(out, "  FAILED: {}", ids.join(", "));
        }
        let _ = writeln!(out, "{}", "=".repeat(70));
        out
    }
}
