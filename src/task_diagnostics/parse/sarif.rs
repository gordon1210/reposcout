use super::{
    ParsedTaskDiagnostics, TaskDiagnosticError, base, clean, mark_record_limit, position, severity,
};
use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::fmt;

pub(super) fn parse(
    content: &str,
    parsed: &mut ParsedTaskDiagnostics,
) -> Result<(), TaskDiagnosticError> {
    let mut version = false;
    let mut runs = false;
    let mut deserializer = serde_json::Deserializer::from_str(content);
    let result = Document {
        parsed,
        version: &mut version,
        runs: &mut runs,
    }
    .deserialize(&mut deserializer);
    if result.is_err() {
        parsed.evidence.parse_errors += 1;
    }
    if !version || !runs {
        return Err(if result.is_err() {
            TaskDiagnosticError::MalformedInput
        } else {
            TaskDiagnosticError::UnsupportedFormat
        });
    }
    if result.is_ok() && deserializer.end().is_err() {
        parsed.evidence.parse_errors += 1;
    }
    if parsed.records.is_empty() && parsed.evidence.parse_errors > 0 {
        Err(TaskDiagnosticError::MalformedInput)
    } else {
        Ok(())
    }
}

struct Document<'a> {
    parsed: &'a mut ParsedTaskDiagnostics,
    version: &'a mut bool,
    runs: &'a mut bool,
}
impl<'de> DeserializeSeed<'de> for Document<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for Document<'_> {
    type Value = ();
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SARIF document")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "version" => {
                    *self.version = map.next_value::<String>()? == "2.1.0";
                }
                "runs" => {
                    *self.runs = true;
                    map.next_value_seed(Runs(self.parsed))?;
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
struct Runs<'a>(&'a mut ParsedTaskDiagnostics);
impl<'de> DeserializeSeed<'de> for Runs<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_seq(self)
    }
}
impl<'de> Visitor<'de> for Runs<'_> {
    type Value = ();
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SARIF runs")
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> Result<(), S::Error> {
        while sequence.next_element_seed(Run(self.0))?.is_some() {}
        Ok(())
    }
}
struct Run<'a>(&'a mut ParsedTaskDiagnostics);
impl<'de> DeserializeSeed<'de> for Run<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(self)
    }
}
impl<'de> Visitor<'de> for Run<'_> {
    type Value = ();
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SARIF run")
    }
    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
        let start = self.0.records.len();
        let mut tool = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "tool" => {
                    let value: Value = map.next_value()?;
                    tool = value["driver"]["name"].as_str().map(|s| clean(s, 128));
                }
                "results" => {
                    let result = map.next_value_seed(Results(self.0));
                    for record in &mut self.0.records[start..] {
                        record.tool.clone_from(&tool);
                    }
                    result?;
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        for record in &mut self.0.records[start..] {
            record.tool.clone_from(&tool);
        }
        Ok(())
    }
}
struct Results<'a>(&'a mut ParsedTaskDiagnostics);
impl<'de> DeserializeSeed<'de> for Results<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_seq(self)
    }
}
impl<'de> Visitor<'de> for Results<'_> {
    type Value = ();
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SARIF results")
    }
    fn visit_seq<S: SeqAccess<'de>>(self, mut sequence: S) -> Result<(), S::Error> {
        loop {
            if self.0.records.len() >= self.0.limits.records {
                while sequence.next_element::<IgnoredAny>()?.is_some() {
                    mark_record_limit(self.0);
                }
                break;
            }
            let Some(value) = sequence.next_element::<Value>()? else {
                break;
            };
            result(&value, self.0);
        }
        Ok(())
    }
}
fn result(value: &Value, parsed: &mut ParsedTaskDiagnostics) {
    let Some(message) = value["message"]["text"]
        .as_str()
        .or_else(|| value["message"]["markdown"].as_str())
    else {
        parsed.evidence.parse_errors += 1;
        return;
    };
    let Some(locations) = value["locations"].as_array() else {
        parsed.evidence.ignored_records += 1;
        return;
    };
    for location in locations {
        if parsed.records.len() >= parsed.limits.records {
            mark_record_limit(parsed);
            return;
        }
        let physical = &location["physicalLocation"];
        let Some(path) = physical["artifactLocation"]["uri"].as_str() else {
            parsed.evidence.parse_errors += 1;
            continue;
        };
        let mut record = base(path, message, "high");
        if physical["artifactLocation"].get("uriBaseId").is_some() {
            record.reason = Some("unsupported-uri-base".into());
        }
        record.code = value["ruleId"].as_str().map(|s| clean(s, 128));
        record.severity = severity(Some(value["level"].as_str().unwrap_or("warning")));
        let region = &physical["region"];
        record.line = position(&region["startLine"]);
        record.column = position(&region["startColumn"]);
        record.end_line = position(&region["endLine"]);
        record.end_column = position(&region["endColumn"]);
        parsed.records.push(record);
    }
}
