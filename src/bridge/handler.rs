use axum::{Router, routing::get, extract::Query, Json};
use std::collections::HashMap;
use tokio::sync::mpsc;

pub struct BridgeServer {
    #[allow(dead_code)]
    tx: mpsc::UnboundedSender<String>,
    #[allow(dead_code)]
    port: u16,
}

impl BridgeServer {
    pub fn new(tx: mpsc::UnboundedSender<String>, port: u16) -> Self {
        Self { tx, port }
    }

    pub async fn start(&mut self) -> Result<(), anyhow::Error> {
        let tx = self.tx.clone();
        let app = Router::new()
            .route("/search", get(search_handler))
            .with_state(tx);

        let addr = format!("127.0.0.1:{}", self.port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Ok(())
    }
}

async fn search_handler(
    Query(params): Query<HashMap<String, String>>,
    axum::extract::State(tx): axum::extract::State<mpsc::UnboundedSender<String>>,
) -> Json<serde_json::Value> {
    let query = params.get("q").cloned().unwrap_or_default();
    let _ = tx.send(query.clone());

    Json(serde_json::json!({
        "success": true,
        "query": query
    }))
}
