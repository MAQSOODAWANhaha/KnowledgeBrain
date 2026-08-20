use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::reconcile::{normalize_quote, quote_in_body};
use super::types::ExtractionReport;

#[derive(Debug, Clone, Deserialize)]
pub struct GoldenExpected {
    pub clauses: Vec<ExpectedClause>,
    #[serde(default)]
    pub absent_quotes: Vec<String>,
    #[serde(default)]
    pub thresholds: EvaluationThresholds,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExpectedClause {
    pub quote: String,
    #[serde(default)]
    pub accepted_aliases: Vec<String>,
    pub family: String,
    pub must: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvaluationThresholds {
    #[serde(default = "one")]
    pub quote_validity: f64,
    #[serde(default)]
    pub unsupported: usize,
    #[serde(default = "one")]
    pub precision: f64,
    #[serde(default = "one")]
    pub technical_precision: f64,
    #[serde(default = "one")]
    pub commercial_precision: f64,
    #[serde(default = "one")]
    pub technical_recall: f64,
    #[serde(default = "one")]
    pub commercial_recall: f64,
    #[serde(default = "one")]
    pub family_accuracy: f64,
    #[serde(default = "one")]
    pub must_accuracy: f64,
    #[serde(default = "zero")]
    pub duplicate_rate_max: f64,
    #[serde(default)]
    pub false_positive_max: usize,
}

impl Default for EvaluationThresholds {
    fn default() -> Self {
        Self {
            quote_validity: 1.0,
            unsupported: 0,
            precision: 1.0,
            technical_precision: 1.0,
            commercial_precision: 1.0,
            technical_recall: 1.0,
            commercial_recall: 1.0,
            family_accuracy: 1.0,
            must_accuracy: 1.0,
            duplicate_rate_max: 0.0,
            false_positive_max: 0,
        }
    }
}

fn one() -> f64 {
    1.0
}

fn zero() -> f64 {
    0.0
}

#[derive(Debug, Clone, Serialize)]
pub struct EvaluationMetrics {
    pub expected: usize,
    pub actual: usize,
    pub assigned: usize,
    pub false_positives: usize,
    pub quote_validity: f64,
    pub unsupported: usize,
    pub precision: f64,
    pub recall: f64,
    pub f1: f64,
    pub technical_precision: f64,
    pub technical_recall: f64,
    pub commercial_precision: f64,
    pub commercial_recall: f64,
    pub family_accuracy: f64,
    pub must_accuracy: f64,
    pub duplicate_rate: f64,
    pub absent_quote_violations: Vec<String>,
    pub passed: bool,
    pub failures: Vec<String>,
}

pub fn evaluate(report: &ExtractionReport, expected: &GoldenExpected) -> EvaluationMetrics {
    let spans: HashMap<_, _> = report
        .sections
        .iter()
        .flat_map(|section| section.spans.iter())
        .map(|span| (span.id.as_str(), span.body.as_str()))
        .collect();
    let valid = report
        .clauses
        .iter()
        .filter(|clause| {
            spans
                .get(clause.span_id.as_str())
                .is_some_and(|body| quote_in_body(&clause.quote, body))
        })
        .count();
    let unsupported = report.clauses.len().saturating_sub(valid);
    let quote_validity = ratio(valid, report.clauses.len());

    let adjacency: Vec<Vec<usize>> = expected
        .clauses
        .iter()
        .map(|expected_clause| {
            report
                .clauses
                .iter()
                .enumerate()
                .filter(|(_, actual)| expected_clause.matches(&actual.quote))
                .map(|(actual_idx, _)| actual_idx)
                .collect()
        })
        .collect();
    let mut expected_order: Vec<_> = (0..expected.clauses.len()).collect();
    expected_order.sort_by_key(|expected_idx| adjacency[*expected_idx].len());
    let mut matched_actual = vec![None; report.clauses.len()];
    for expected_idx in expected_order {
        let mut seen_actual = vec![false; report.clauses.len()];
        augment_assignment(
            expected_idx,
            &adjacency,
            &mut matched_actual,
            &mut seen_actual,
        );
    }
    let assignments: Vec<_> = matched_actual
        .into_iter()
        .enumerate()
        .filter_map(|(actual_idx, expected_idx)| expected_idx.map(|e| (e, actual_idx)))
        .collect();

    let assigned = assignments.len();
    let false_positives = report.clauses.len().saturating_sub(assigned);
    let precision = ratio(assigned, report.clauses.len());
    let recall = ratio(assigned, expected.clauses.len());
    let f1 = if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    };
    let family_accuracy = ratio(
        assignments
            .iter()
            .filter(|(e, a)| expected.clauses[*e].family == report.clauses[*a].family)
            .count(),
        assigned,
    );
    let must_accuracy = ratio(
        assignments
            .iter()
            .filter(|(e, a)| expected.clauses[*e].must == report.clauses[*a].must)
            .count(),
        assigned,
    );
    let (technical_precision, technical_recall) =
        family_metrics("technical", report, expected, &assignments);
    let (commercial_precision, commercial_recall) =
        family_metrics("commercial", report, expected, &assignments);
    let unique: HashSet<_> = report
        .clauses
        .iter()
        .map(|clause| (clause.family.as_str(), normalize_quote(&clause.quote)))
        .collect();
    let duplicate_rate = ratio(
        report.clauses.len().saturating_sub(unique.len()),
        report.clauses.len(),
    );
    let absent_quote_violations: Vec<_> = expected
        .absent_quotes
        .iter()
        .filter(|absent| {
            report.clauses.iter().any(|clause| {
                clause.quote.contains(absent.as_str()) || clause.text.contains(absent.as_str())
            })
        })
        .cloned()
        .collect();

    let thresholds = &expected.thresholds;
    let mut failures = Vec::new();
    check_min(
        &mut failures,
        "quote_validity",
        quote_validity,
        thresholds.quote_validity,
    );
    check_max_usize(
        &mut failures,
        "unsupported",
        unsupported,
        thresholds.unsupported,
    );
    check_min(&mut failures, "precision", precision, thresholds.precision);
    check_min(
        &mut failures,
        "technical_precision",
        technical_precision,
        thresholds.technical_precision,
    );
    check_min(
        &mut failures,
        "commercial_precision",
        commercial_precision,
        thresholds.commercial_precision,
    );
    check_min(
        &mut failures,
        "technical_recall",
        technical_recall,
        thresholds.technical_recall,
    );
    check_min(
        &mut failures,
        "commercial_recall",
        commercial_recall,
        thresholds.commercial_recall,
    );
    check_min(
        &mut failures,
        "family_accuracy",
        family_accuracy,
        thresholds.family_accuracy,
    );
    check_min(
        &mut failures,
        "must_accuracy",
        must_accuracy,
        thresholds.must_accuracy,
    );
    if duplicate_rate > thresholds.duplicate_rate_max + f64::EPSILON {
        failures.push(format!(
            "duplicate_rate {duplicate_rate:.4} >= {:.4}",
            thresholds.duplicate_rate_max
        ));
    }
    check_max_usize(
        &mut failures,
        "false_positives",
        false_positives,
        thresholds.false_positive_max,
    );
    if !absent_quote_violations.is_empty() {
        failures.push(format!(
            "absent quote violations: {}",
            absent_quote_violations.join(", ")
        ));
    }

    EvaluationMetrics {
        expected: expected.clauses.len(),
        actual: report.clauses.len(),
        assigned,
        false_positives,
        quote_validity,
        unsupported,
        precision,
        recall,
        f1,
        technical_precision,
        technical_recall,
        commercial_precision,
        commercial_recall,
        family_accuracy,
        must_accuracy,
        duplicate_rate,
        absent_quote_violations,
        passed: failures.is_empty(),
        failures,
    }
}

impl ExpectedClause {
    fn matches(&self, actual: &str) -> bool {
        std::iter::once(self.quote.as_str())
            .chain(self.accepted_aliases.iter().map(String::as_str))
            .any(|expected| evaluation_quote(expected) == evaluation_quote(actual))
    }
}

fn evaluation_quote(quote: &str) -> String {
    normalize_quote(quote)
        .trim_end_matches(['.', '。', ';', '；', ',', '，'])
        .to_string()
}

fn augment_assignment(
    expected_idx: usize,
    adjacency: &[Vec<usize>],
    matched_actual: &mut [Option<usize>],
    seen_actual: &mut [bool],
) -> bool {
    for &actual_idx in &adjacency[expected_idx] {
        if seen_actual[actual_idx] {
            continue;
        }
        seen_actual[actual_idx] = true;
        if matched_actual[actual_idx].is_none_or(|previous_expected| {
            augment_assignment(previous_expected, adjacency, matched_actual, seen_actual)
        }) {
            matched_actual[actual_idx] = Some(expected_idx);
            return true;
        }
    }
    false
}

fn family_metrics(
    family: &str,
    report: &ExtractionReport,
    expected: &GoldenExpected,
    assignments: &[(usize, usize)],
) -> (f64, f64) {
    let actual_total = report
        .clauses
        .iter()
        .filter(|clause| clause.family == family)
        .count();
    let expected_total = expected
        .clauses
        .iter()
        .filter(|clause| clause.family == family)
        .count();
    let correct = assignments
        .iter()
        .filter(|(e, a)| {
            expected.clauses[*e].family == family && report.clauses[*a].family == family
        })
        .count();
    (ratio(correct, actual_total), ratio(correct, expected_total))
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        1.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn check_min(failures: &mut Vec<String>, name: &str, value: f64, threshold: f64) {
    if value + f64::EPSILON < threshold {
        failures.push(format!("{name} {value:.4} < {threshold:.4}"));
    }
}

fn check_max_usize(failures: &mut Vec<String>, name: &str, value: usize, threshold: usize) {
    if value > threshold {
        failures.push(format!("{name} {value} > {threshold}"));
    }
}
