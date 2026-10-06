use crate::acceptance::support::{assert_fixture_bounds, probe};
use crate::efficiency::support::EvidenceFragment;
use crate::support::Fixture;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt::Write as _;

const INVOICES: &str = include_str!("invoices.py");
const MODELS: &str = include_str!("models.py");
const BROKEN_HELPER: &str = include_str!("rounding.py");
const REGRESSION: &str = include_str!("test_invoices.py");

const IMPORTS: &str = "from src.models import Invoice\nfrom src.money.rounding import rounded_discount as discount_cent\n";
const SWITCHED_IMPORTS: &str = "from src.models import Invoice\nfrom src.money.settlement_rounding import settlement_discount as discount_cent\n";
const TARGET: &str = r#"def net_due(invoice: Invoice) -> int:
    """Return payable cents after applying the invoice's basis-point discount."""
    discounted_cents = discount_cent(invoice.amount_cents, invoice.discount_bps)
    return invoice.amount_cents - discounted_cents
"#;
const REPAIRED_HELPER: &str = r#"def rounded_discount(amount: int, bps: int) -> int:
    """Round a nonnegative cent amount's basis-point discount to cents."""
    if amount < 0 or not 0 <= bps <= 10000:
        raise ValueError("discount requires cents >= 0 and basis points in [0, 10000]")
    return (amount * bps + 5000) // 10000
"#;
const SWITCHED_HELPER: &str = r#"def settlement_discount(amount: int, bps: int) -> int:
    """Round the cent discount recorded by the settlement workflow."""
    if amount < 0 or not 0 <= bps <= 10000:
        raise ValueError("discount requires cents >= 0 and basis points in [0, 10000]")
    return round(amount * bps / 10000)
"#;
const DEMO: &str = r"def net_due(invoice):
    discount = (invoice.amount_cents * invoice.discount_bps + 5000) // 10000
    return invoice.amount_cents - discount
";
const NOISE_HANDLER: &str = r#"

def handle_invoice_summary(request, records):
    """Select dashboard records without evaluating payable invoice amounts."""
    selected = records
    if request.get("currency"):
        selected = [record for record in selected
                    if record["currency"] == request["currency"]]
    if request.get("customer_id"):
        selected = [record for record in selected
                    if record["customer_id"] == request["customer_id"]]
    return {"status": 200, "body": invoice_balance_report(selected)}
"#;

/// Each literal span was selected before consulting any product response.
pub(super) struct RequiredFragment {
    pub(super) obligation: &'static str,
    pub(super) fragment: EvidenceFragment<'static>,
    pub(super) start_line: usize,
    pub(super) end_line: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Variant {
    Broken,
    Repaired,
    Switched,
    Counterfeit,
    Noise,
}

pub(super) struct InvoiceCase {
    pub(super) fixture: Fixture,
    authored: BTreeMap<String, String>,
    pub(super) required: Vec<RequiredFragment>,
    pub(super) variant: Variant,
}

impl InvoiceCase {
    pub(super) fn new(label: &str, variant: Variant) -> Self {
        let mut case = Self {
            fixture: Fixture::new(label),
            authored: BTreeMap::new(),
            required: packet(variant),
            variant,
        };
        let invoices = match variant {
            Variant::Switched => INVOICES.replacen(IMPORTS, SWITCHED_IMPORTS, 1),
            Variant::Noise => {
                let mut source = INVOICES
                    .replace("paginate_invoices", "invoice_page")
                    .replace("export_invoice_rows", "ledger_export_rows")
                    .replace("preview_invoice", "draft_invoice_preview")
                    .replace("invoice_status_report", "invoice_balance_report");
                source.push_str(NOISE_HANDLER);
                source
            }
            _ => INVOICES.to_owned(),
        };
        case.write("src/invoices.py", &invoices);
        case.write("src/models.py", MODELS);
        case.write(
            "src/money/rounding.py",
            if matches!(variant, Variant::Repaired | Variant::Switched) {
                REPAIRED_HELPER
            } else {
                BROKEN_HELPER
            },
        );
        if variant == Variant::Switched {
            case.write("src/money/settlement_rounding.py", SWITCHED_HELPER);
        }
        case.write("demo/invoices.py", DEMO);
        let regression = if variant == Variant::Counterfeit {
            REGRESSION.replacen(
                "from src.invoices import net_due",
                "from demo.invoices import net_due",
                1,
            )
        } else {
            REGRESSION.to_owned()
        };
        case.write("tests/test_invoices.py", &regression);
        case.write(
            "pyproject.toml",
            "[tool.pytest.ini_options]\ntestpaths = [\"tests\"]\npython_files = [\"test_*.py\"]\n",
        );
        case.freeze_packet();
        assert_fixture_bounds(&case.fixture);
        case
    }

    fn write(&mut self, path: &str, source: &str) {
        self.fixture.write(path, source);
        self.authored.insert(path.to_owned(), source.to_owned());
    }

    fn freeze_packet(&self) {
        let lines: usize = self
            .required
            .iter()
            .map(|piece| nonblank(piece.fragment.source))
            .sum();
        let escaped_bytes: usize = self
            .required
            .iter()
            .map(|piece| serde_json::to_vec(piece.fragment.source).unwrap().len())
            .sum();
        assert!(lines <= 42, "frozen I packet has {lines} necessary lines");
        assert!(
            escaped_bytes <= 3 * 1024,
            "frozen I packet exceeds 3 KiB escaped source"
        );
        assert!(
            nonblank(&self.authored["src/invoices.py"]) > 56,
            "the natural invoice module must exceed the whole-episode source ceiling"
        );
        for piece in &self.required {
            // The counterfeit control deliberately lacks the genuine production-path check.
            if self.variant == Variant::Counterfeit && piece.obligation == "production regression" {
                continue;
            }
            let source = self.source(piece.fragment.path);
            let selected: String = source
                .lines()
                .skip(piece.start_line - 1)
                .take(piece.end_line - piece.start_line + 1)
                .fold(String::new(), |mut selected, line| {
                    writeln!(selected, "{line}").unwrap();
                    selected
                });
            assert_eq!(
                selected, piece.fragment.source,
                "frozen span for {}",
                piece.obligation
            );
        }
        let description: Vec<_> = self
            .required
            .iter()
            .map(|piece| {
                json!({
                    "obligation": piece.obligation,
                    "path": piece.fragment.path,
                    "start_line": piece.start_line,
                    "end_line": piece.end_line,
                    "source": piece.fragment.source,
                    "content_hash": self.hash(piece.fragment.path),
                })
            })
            .collect();
        std::fs::write(
            self.fixture.state_path().join("invoice-frozen-packet.json"),
            serde_json::to_vec_pretty(&description).unwrap(),
        )
        .unwrap();
    }

    pub(super) fn source(&self, path: &str) -> &str {
        &self.authored[path]
    }

    pub(super) fn has_path(&self, path: &str) -> bool {
        self.authored.contains_key(path)
    }

    pub(super) fn hash(&self, path: &str) -> String {
        let mut hash = String::with_capacity(64);
        for byte in Sha256::digest(self.source(path).as_bytes()) {
            write!(hash, "{byte:02x}").unwrap();
        }
        hash
    }

    pub(super) fn assert_truth(&self) -> Value {
        let actual = probe(
            &self.fixture,
            r#"
import json
import runpy
from src.invoices import net_due, discount_cent
from src.models import Invoice
payables = [net_due(Invoice(amount_cents=amount, discount_bps=bps))
            for amount, bps in [(1000, 1000), (1005, 1000), (1005, 0), (1005, 10000)]]
regression = runpy.run_path("tests/test_invoices.py")["test_net_due_rounds_half_up"]
try:
    regression()
    regression_passed = True
except AssertionError:
    regression_passed = False
print(json.dumps({"payables": payables, "active_module": discount_cent.__module__,
                  "active_name": discount_cent.__name__, "regression_passed": regression_passed}))
"#,
        );
        let repaired = self.variant == Variant::Repaired;
        assert_eq!(
            actual["payables"],
            if repaired {
                json!([900, 904, 1005, 0])
            } else {
                json!([900, 905, 1005, 0])
            },
            "literal payable examples"
        );
        assert_eq!(
            actual["regression_passed"],
            json!(repaired || self.variant == Variant::Counterfeit),
            "genuine regression fails faulty production; counterfeit passes only the demo"
        );
        assert_eq!(
            actual["active_module"],
            if self.variant == Variant::Switched {
                json!("src.money.settlement_rounding")
            } else {
                json!("src.money.rounding")
            }
        );
        assert_eq!(
            actual["active_name"],
            if self.variant == Variant::Switched {
                json!("settlement_discount")
            } else {
                json!("rounded_discount")
            }
        );
        actual
    }
}

fn packet(variant: Variant) -> Vec<RequiredFragment> {
    let (binding, helper_path, helper) = match variant {
        Variant::Switched => (
            SWITCHED_IMPORTS,
            "src/money/settlement_rounding.py",
            SWITCHED_HELPER,
        ),
        Variant::Repaired => (IMPORTS, "src/money/rounding.py", REPAIRED_HELPER),
        _ => (IMPORTS, "src/money/rounding.py", BROKEN_HELPER),
    };
    [
        ("active module binding", "src/invoices.py", binding, 1, 2),
        ("cent and basis-point fields", "src/models.py", MODELS, 1, 9),
        ("known payable definition", "src/invoices.py", TARGET, 5, 8),
        ("active rounding change site", helper_path, helper, 1, 5),
        (
            "production regression",
            "tests/test_invoices.py",
            REGRESSION,
            1,
            9,
        ),
    ]
    .into_iter()
    .map(
        |(obligation, path, source, start_line, end_line)| RequiredFragment {
            obligation,
            fragment: EvidenceFragment { path, source },
            start_line,
            end_line,
        },
    )
    .collect()
}

fn nonblank(source: &str) -> usize {
    source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
}
