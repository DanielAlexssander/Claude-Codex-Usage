//! Status público do Claude (status.claude.com, página Statuspage).
//!
//! Consulta `summary.json` a cada poucos minutos e resume o que importa para o painel:
//! o indicador geral, os incidentes em aberto e os componentes fora do normal.
//! É uma página pública: nenhuma credencial é enviada.

use serde::Serialize;
use serde_json::Value;

pub const SUMMARY_URL: &str = "https://status.claude.com/api/v2/summary.json";
pub const PAGE_URL: &str = "https://status.claude.com";

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Incident {
    pub name: String,
    /// investigating | identified | monitoring | ...
    pub status: String,
    /// none | minor | major | critical
    pub impact: String,
    pub url: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    pub name: String,
    /// degraded_performance | partial_outage | major_outage | under_maintenance
    pub status: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    /// none | minor | major | critical | maintenance
    pub indicator: String,
    pub description: String,
    pub incidents: Vec<Incident>,
    pub components: Vec<Component>,
    pub url: String,
    /// epoch ms da última consulta bem-sucedida
    pub checked_at: i64,
}

impl ServiceStatus {
    pub fn is_degraded(&self) -> bool {
        !self.incidents.is_empty() || !self.components.is_empty() || !matches!(self.indicator.as_str(), "" | "none")
    }
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}

pub fn parse(v: &Value, now_ms: i64) -> ServiceStatus {
    let status = v.get("status").cloned().unwrap_or(Value::Null);
    let incidents = v
        .get("incidents")
        .and_then(|i| i.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|i| !matches!(s(i, "status").as_str(), "resolved" | "postmortem" | "completed"))
                .map(|i| Incident {
                    name: s(i, "name"),
                    status: s(i, "status"),
                    impact: s(i, "impact"),
                    // link na própria status.claude.com (o encurtador stspg.io não está na lista de links permitidos)
                    url: i.get("id").and_then(|x| x.as_str()).map(|id| format!("{PAGE_URL}/incidents/{id}")),
                    updated_at: i.get("updated_at").and_then(|x| x.as_str()).map(str::to_string),
                })
                .filter(|i| !i.name.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let components = v
        .get("components")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                // grupos repetem o status dos filhos
                .filter(|c| !c.get("group").and_then(|g| g.as_bool()).unwrap_or(false))
                .filter(|c| !matches!(s(c, "status").as_str(), "" | "operational"))
                .map(|c| Component { name: s(c, "name"), status: s(c, "status") })
                .collect()
        })
        .unwrap_or_default();
    ServiceStatus {
        indicator: s(&status, "indicator"),
        description: s(&status, "description"),
        incidents,
        components,
        url: PAGE_URL.into(),
        checked_at: now_ms,
    }
}

pub async fn fetch(client: &reqwest::Client, now_ms: i64) -> Result<ServiceStatus, String> {
    let resp = client
        .get(SUMMARY_URL)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(parse(&v, now_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_statuspage_summary() {
        let v: Value = serde_json::from_str(
            r#"{
              "page": {"id": "x", "name": "Claude", "url": "https://status.claude.com"},
              "status": {"indicator": "minor", "description": "Partially Degraded Service"},
              "components": [
                {"id": "a", "name": "claude.ai", "status": "operational", "group": false},
                {"id": "b", "name": "Claude Code", "status": "degraded_performance", "group": false},
                {"id": "g", "name": "Grupo", "status": "partial_outage", "group": true}
              ],
              "incidents": [
                {"id": "abc", "name": "Elevated errors on Claude Opus", "status": "investigating", "impact": "minor",
                 "shortlink": "https://stspg.io/abc", "updated_at": "2026-10-21T12:00:00Z"},
                {"name": "Old one", "status": "resolved", "impact": "major"}
              ],
              "scheduled_maintenances": []
            }"#,
        )
        .unwrap();
        let st = parse(&v, 1);
        assert_eq!(st.indicator, "minor");
        assert_eq!(st.incidents.len(), 1);
        assert_eq!(st.incidents[0].url.as_deref(), Some("https://status.claude.com/incidents/abc"));
        assert_eq!(st.components.len(), 1);
        assert_eq!(st.components[0].name, "Claude Code");
        assert!(st.is_degraded());

        let ok = parse(&serde_json::json!({"status": {"indicator": "none", "description": "All Systems Operational"}, "components": [], "incidents": []}), 1);
        assert!(!ok.is_degraded());
    }
}
