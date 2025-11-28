// =============================================================================
// GOOGLE DOCS CLIENT WITH SERVICE ACCOUNT AUTHENTICATION
// =============================================================================
//
// This module provides a client for fetching Google Docs content, including
// support for documents with multiple tabs.
//
// **Why not use URL Context?**
// Gemini's URL Context tool explicitly does NOT support Google Workspace files.
// From the docs: "The following content types are not supported: Google
// workspace files like Google docs or spreadsheets"
//
// **Authentication Options:**
//
// 1. **Public Export (simplest, single tab only):**
//    - Document must be shared as "Anyone with the link can view"
//    - No API key or OAuth required
//    - Only fetches the first/default tab
//
// 2. **Service Account (recommended for multi-tab docs):**
//    - Create a service account in Google Cloud Console
//    - Share the document with the service account email
//    - Fetches ALL tabs in the document
//
// **Setup Instructions for Service Account:**
//
// 1. Go to Google Cloud Console: https://console.cloud.google.com/
// 2. Create a new project (or select existing)
// 3. Enable the Google Docs API:
//    - Go to "APIs & Services" > "Library"
//    - Search for "Google Docs API" and enable it
// 4. Create a Service Account:
//    - Go to "APIs & Services" > "Credentials"
//    - Click "Create Credentials" > "Service Account"
//    - Give it a name (e.g., "docs-reader")
//    - No need to grant roles for now
// 5. Create a JSON key:
//    - Click on the service account you created
//    - Go to "Keys" tab
//    - "Add Key" > "Create new key" > JSON
//    - Save the downloaded JSON file securely
// 6. Share your Google Doc:
//    - Open your Google Doc
//    - Click "Share"
//    - Add the service account email (looks like: name@project.iam.gserviceaccount.com)
//    - Give it "Viewer" access
// 7. Set environment variables:
//    - `GOOGLE_SERVICE_ACCOUNT_KEY` - Path to the JSON key file
//      OR
//    - `GOOGLE_SERVICE_ACCOUNT_JSON` - The JSON content directly (for deployment)
//    - `GOOGLE_DOC_IDS` - Comma-separated list of document IDs
//
// **Environment Variables:**
// - `GOOGLE_SERVICE_ACCOUNT_KEY` - Path to service account JSON file
// - `GOOGLE_SERVICE_ACCOUNT_JSON` - Service account JSON content (alternative)
// - `GOOGLE_DOC_IDS` - Comma-separated list of document IDs to pre-fetch

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

use crate::core::ai::models::{AiTool, FunctionDef, FunctionParameters, PropertyDef};
use crate::core::ai::FunctionCallHandler;
use async_trait::async_trait;

// =============================================================================
// SERVICE ACCOUNT AUTHENTICATION
// =============================================================================

/// Service account credentials from the JSON key file.
#[derive(Debug, Clone, Deserialize)]
struct ServiceAccountCredentials {
    /// The service account email (used as issuer in JWT).
    client_email: String,

    /// The private key in PEM format.
    private_key: String,

    /// The token URI (where to exchange JWT for access token).
    token_uri: String,
}

/// JWT claims for Google OAuth2.
#[derive(Debug, Serialize)]
struct JwtClaims {
    /// Issuer (service account email).
    iss: String,

    /// Scope (what APIs we want access to).
    scope: String,

    /// Audience (token endpoint).
    aud: String,

    /// Issued at (Unix timestamp).
    iat: u64,

    /// Expiration (Unix timestamp, max 1 hour from iat).
    exp: u64,
}

/// Response from Google's token endpoint.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[allow(dead_code)]
    expires_in: u64,
    #[allow(dead_code)]
    token_type: String,
}

/// Cached access token with expiration.
struct CachedToken {
    token: String,
    expires_at: SystemTime,
}

/// Authenticator that handles OAuth2 with service account credentials.
pub struct ServiceAccountAuth {
    credentials: ServiceAccountCredentials,
    client: Client,
    cached_token: Arc<RwLock<Option<CachedToken>>>,
}

impl ServiceAccountAuth {
    /// Creates a new authenticator from a JSON key file path.
    pub async fn from_file(path: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let content = tokio::fs::read_to_string(path).await?;
        Self::from_json(&content)
    }

    /// Creates a new authenticator from JSON content.
    pub fn from_json(json: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let credentials: ServiceAccountCredentials = serde_json::from_str(json)?;
        Ok(Self {
            credentials,
            client: Client::new(),
            cached_token: Arc::new(RwLock::new(None)),
        })
    }

    /// Creates from environment variables.
    pub async fn from_env() -> Result<Self, Box<dyn Error + Send + Sync>> {
        if let Ok(path) = std::env::var("GOOGLE_SERVICE_ACCOUNT_KEY") {
            return Self::from_file(&path).await;
        }

        if let Ok(json) = std::env::var("GOOGLE_SERVICE_ACCOUNT_JSON") {
            return Self::from_json(&json);
        }

        Err("Neither GOOGLE_SERVICE_ACCOUNT_KEY nor GOOGLE_SERVICE_ACCOUNT_JSON is set.".into())
    }

    /// Gets a valid access token, refreshing if necessary.
    pub async fn get_access_token(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        // Check if we have a valid cached token
        {
            let cached = self.cached_token.read().await;
            if let Some(token) = cached.as_ref() {
                if token.expires_at > SystemTime::now() + Duration::from_secs(60) {
                    return Ok(token.token.clone());
                }
            }
        }

        // Need to refresh the token
        let new_token = self.fetch_new_token().await?;

        // Cache it
        {
            let mut cached = self.cached_token.write().await;
            *cached = Some(CachedToken {
                token: new_token.clone(),
                expires_at: SystemTime::now() + Duration::from_secs(55 * 60),
            });
        }

        Ok(new_token)
    }

    /// Fetches a new access token from Google.
    async fn fetch_new_token(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

        let claims = JwtClaims {
            iss: self.credentials.client_email.clone(),
            scope: "https://www.googleapis.com/auth/documents.readonly".to_string(),
            aud: self.credentials.token_uri.clone(),
            iat: now,
            exp: now + 3600,
        };

        let header = Header::new(Algorithm::RS256);
        let key = EncodingKey::from_rsa_pem(self.credentials.private_key.as_bytes())?;
        let jwt = encode(&header, &claims, &key)?;

        let response = self
            .client
            .post(&self.credentials.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &jwt),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(format!("Token exchange failed ({}): {}", status, text).into());
        }

        let token_response: TokenResponse = response.json().await?;
        Ok(token_response.access_token)
    }
}

// =============================================================================
// GOOGLE DOCS API RESPONSE STRUCTURES
// =============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tab {
    tab_properties: TabProperties,
    document_tab: Option<DocumentTab>,
    #[serde(default)]
    child_tabs: Vec<Tab>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TabProperties {
    #[allow(dead_code)]
    tab_id: String,
    title: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DocumentTab {
    body: Option<Body>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Body {
    content: Vec<StructuralElement>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StructuralElement {
    paragraph: Option<Paragraph>,
    table: Option<Table>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Paragraph {
    elements: Vec<ParagraphElement>,
    paragraph_style: Option<ParagraphStyle>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParagraphStyle {
    named_style_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ParagraphElement {
    text_run: Option<TextRun>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TextRun {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Table {
    table_rows: Vec<TableRow>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TableRow {
    table_cells: Vec<TableCell>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TableCell {
    content: Vec<StructuralElement>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Document {
    #[allow(dead_code)]
    document_id: String,
    title: String,
    #[serde(default)]
    tabs: Vec<Tab>,
}

// =============================================================================
// GOOGLE DOCS CLIENT
// =============================================================================

/// Client for fetching Google Docs content with multi-tab support.
pub struct GoogleDocsClient {
    client: Client,
    auth: Option<ServiceAccountAuth>,
}

impl GoogleDocsClient {
    /// Creates a new client for public documents only (no authentication).
    pub fn new() -> Self {
        Self {
            client: Client::new(),
            auth: None,
        }
    }

    /// Creates a client with service account authentication.
    pub fn with_service_account(auth: ServiceAccountAuth) -> Self {
        Self {
            client: Client::new(),
            auth: Some(auth),
        }
    }

    /// Creates a client with service account from environment variables.
    pub async fn with_service_account_from_env() -> Result<Self, Box<dyn Error + Send + Sync>> {
        let auth = ServiceAccountAuth::from_env().await?;
        Ok(Self::with_service_account(auth))
    }

    /// Pre-fetches documents at startup and returns their combined content.
    #[allow(dead_code)]
    pub async fn prefetch_for_system_prompt(
        &self,
        doc_ids: &[&str],
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let mut combined = String::new();

        for doc_id in doc_ids {
            let result = if self.auth.is_some() {
                self.get_all_tabs_text(doc_id).await
            } else {
                self.get_document_text(doc_id).await
            };

            match result {
                Ok(content) => {
                    if !combined.is_empty() {
                        combined.push_str("\n\n---\n\n");
                    }
                    combined.push_str(&content);
                    tracing::info!("Pre-fetched Google Doc: {}", doc_id);
                }
                Err(e) => {
                    tracing::error!("Failed to pre-fetch document {}: {}", doc_id, e);
                }
            }
        }

        if combined.is_empty() {
            Err("Failed to fetch any documents".into())
        } else {
            Ok(combined)
        }
    }

    /// Pre-fetches from GOOGLE_DOC_IDS environment variable.
    #[allow(dead_code)]
    pub async fn prefetch_from_env(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        let doc_ids_str = std::env::var("GOOGLE_DOC_IDS")
            .map_err(|_| "GOOGLE_DOC_IDS environment variable not set")?;

        let doc_ids: Vec<&str> = doc_ids_str.split(',').map(|s| s.trim()).collect();

        if doc_ids.is_empty() {
            return Err("GOOGLE_DOC_IDS is empty".into());
        }

        tracing::info!("Pre-fetching {} Google Doc(s)...", doc_ids.len());
        self.prefetch_for_system_prompt(&doc_ids).await
    }

    /// Extracts the document ID from a Google Docs URL.
    pub fn extract_doc_id(url_or_id: &str) -> Option<String> {
        if url_or_id.contains("docs.google.com") {
            if let Some(start) = url_or_id.find("/document/d/") {
                let after_d = &url_or_id[start + 12..];
                let end = after_d.find('/').unwrap_or(after_d.len());
                let id = &after_d[..end];
                if !id.is_empty() {
                    return Some(id.to_string());
                }
            }
        } else if !url_or_id.contains('/') && !url_or_id.contains(' ') {
            return Some(url_or_id.to_string());
        }
        None
    }

    /// Fetches a document using the public export endpoint (first tab only).
    pub async fn get_document_text(
        &self,
        doc_id_or_url: &str,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let doc_id = Self::extract_doc_id(doc_id_or_url)
            .ok_or_else(|| format!("Could not extract document ID from: {}", doc_id_or_url))?;

        let url = format!(
            "https://docs.google.com/document/d/{}/export?format=txt",
            doc_id
        );

        tracing::debug!("Fetching Google Doc via export: {}", doc_id);

        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(format!(
                "Failed to fetch document ({}): {}. \
                 Make sure the document is shared as 'Anyone with the link can view'.",
                status, text
            )
            .into());
        }

        Ok(response.text().await?)
    }

    /// Fetches ALL tabs from a document using the Google Docs API.
    pub async fn get_all_tabs_text(
        &self,
        doc_id_or_url: &str,
    ) -> Result<String, Box<dyn Error + Send + Sync>> {
        let auth = self.auth.as_ref().ok_or(
            "Service account authentication required for multi-tab documents. \
             Set GOOGLE_SERVICE_ACCOUNT_KEY environment variable.",
        )?;

        let doc_id = Self::extract_doc_id(doc_id_or_url)
            .ok_or_else(|| format!("Could not extract document ID from: {}", doc_id_or_url))?;

        let token = auth.get_access_token().await?;

        let url = format!(
            "https://docs.googleapis.com/v1/documents/{}?includeTabsContent=true",
            doc_id
        );

        tracing::debug!("Fetching Google Doc via API (all tabs): {}", doc_id);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await?;
            return Err(format!(
                "Google Docs API error ({}): {}. \
                 Make sure the document is shared with your service account email.",
                status, text
            )
            .into());
        }

        let document: Document = response.json().await?;

        let mut combined = format!("# {}\n\n", document.title);

        if document.tabs.is_empty() {
            combined.push_str("(No tabs found in document)\n");
        } else {
            self.extract_all_tabs_text(&document.tabs, &mut combined, 0);
        }

        tracing::info!(
            "Fetched Google Doc '{}' with {} top-level tab(s): {} chars",
            document.title,
            document.tabs.len(),
            combined.len()
        );

        Ok(combined)
    }

    fn extract_all_tabs_text(&self, tabs: &[Tab], output: &mut String, depth: usize) {
        for tab in tabs {
            let indent = "#".repeat(depth + 2);
            output.push_str(&format!("\n{} {}\n\n", indent, tab.tab_properties.title));

            if let Some(doc_tab) = &tab.document_tab {
                if let Some(body) = &doc_tab.body {
                    self.extract_body_text(body, output);
                }
            }

            if !tab.child_tabs.is_empty() {
                self.extract_all_tabs_text(&tab.child_tabs, output, depth + 1);
            }
        }
    }

    fn extract_body_text(&self, body: &Body, output: &mut String) {
        for element in &body.content {
            self.extract_element_text(element, output);
        }
    }

    fn extract_element_text(&self, element: &StructuralElement, output: &mut String) {
        if let Some(paragraph) = &element.paragraph {
            if let Some(style) = &paragraph.paragraph_style {
                if let Some(style_type) = &style.named_style_type {
                    match style_type.as_str() {
                        "HEADING_1" => output.push_str("### "),
                        "HEADING_2" => output.push_str("#### "),
                        "HEADING_3" => output.push_str("##### "),
                        _ => {}
                    }
                }
            }

            for para_element in &paragraph.elements {
                if let Some(text_run) = &para_element.text_run {
                    if let Some(content) = &text_run.content {
                        output.push_str(content);
                    }
                }
            }
        }

        if let Some(table) = &element.table {
            output.push('\n');
            for row in &table.table_rows {
                let mut cells: Vec<String> = Vec::new();
                for cell in &row.table_cells {
                    let mut cell_text = String::new();
                    for cell_element in &cell.content {
                        self.extract_element_text(cell_element, &mut cell_text);
                    }
                    cells.push(cell_text.trim().to_string());
                }
                output.push_str(&format!("| {} |\n", cells.join(" | ")));
            }
            output.push('\n');
        }
    }
}

impl Default for GoogleDocsClient {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// FUNCTION TOOL HELPERS
// =============================================================================

pub fn read_google_doc_function() -> FunctionDef {
    let mut properties = HashMap::new();

    properties.insert(
        "document_id".to_string(),
        PropertyDef {
            prop_type: "string".to_string(),
            description: Some("The Google Doc document ID or full URL.".to_string()),
            enum_values: None,
        },
    );

    FunctionDef {
        name: "read_google_doc".to_string(),
        description: "Reads the content of a Google Doc including all tabs.".to_string(),
        parameters: FunctionParameters {
            param_type: "object".to_string(),
            properties,
            required: vec!["document_id".to_string()],
        },
    }
}

// =============================================================================
// PROJECT DOCS CONFIGURATION
// =============================================================================

#[derive(Debug, Clone, Default)]
pub struct ProjectDocsConfig {
    pub story_bible_id: Option<String>,
    pub script_id: Option<String>,
    pub writers_notes_id: Option<String>,
    pub additional_docs: Vec<(String, String, String)>,
}

impl ProjectDocsConfig {
    pub fn as_function_tool(&self) -> AiTool {
        let mut properties = HashMap::new();
        let mut doc_names: Vec<String> = Vec::new();
        let mut doc_descriptions: Vec<String> = Vec::new();

        if self.story_bible_id.is_some() {
            doc_names.push("story_bible".to_string());
            doc_descriptions
                .push("story_bible: Character backgrounds, world-building, lore".to_string());
        }
        if self.script_id.is_some() {
            doc_names.push("script".to_string());
            doc_descriptions.push("script: The main script/screenplay".to_string());
        }
        if self.writers_notes_id.is_some() {
            doc_names.push("writers_notes".to_string());
            doc_descriptions
                .push("writers_notes: Ideas, brainstorming, development notes".to_string());
        }
        for (name, _, desc) in &self.additional_docs {
            doc_names.push(name.clone());
            doc_descriptions.push(format!("{}: {}", name, desc));
        }

        properties.insert(
            "document_name".to_string(),
            PropertyDef {
                prop_type: "string".to_string(),
                description: Some(format!(
                    "The name of the document to read. Available:\n{}",
                    doc_descriptions.join("\n")
                )),
                enum_values: Some(doc_names),
            },
        );

        AiTool::FunctionDeclaration(FunctionDef {
            name: "read_project_doc".to_string(),
            description: "Reads a project document by name.".to_string(),
            parameters: FunctionParameters {
                param_type: "object".to_string(),
                properties,
                required: vec!["document_name".to_string()],
            },
        })
    }

    pub fn get_doc_id(&self, name: &str) -> Option<&str> {
        match name {
            "story_bible" => self.story_bible_id.as_deref(),
            "script" => self.script_id.as_deref(),
            "writers_notes" => self.writers_notes_id.as_deref(),
            other => self
                .additional_docs
                .iter()
                .find(|(n, _, _)| n == other)
                .map(|(_, id, _)| id.as_str()),
        }
    }
}

// =============================================================================
// GOOGLE DOCS FUNCTION HANDLER
// =============================================================================

pub struct GoogleDocsFunctionHandler {
    client: GoogleDocsClient,
    project_docs: ProjectDocsConfig,
}

impl GoogleDocsFunctionHandler {
    pub fn new(client: GoogleDocsClient, project_docs: ProjectDocsConfig) -> Self {
        Self {
            client,
            project_docs,
        }
    }

    pub async fn from_env_with_auth() -> Result<Self, Box<dyn Error + Send + Sync>> {
        let client = GoogleDocsClient::with_service_account_from_env().await?;

        let project_docs = ProjectDocsConfig {
            story_bible_id: std::env::var("PROJECT_DOC_STORY_BIBLE").ok(),
            script_id: std::env::var("PROJECT_DOC_SCRIPT").ok(),
            writers_notes_id: std::env::var("PROJECT_DOC_WRITERS_NOTES").ok(),
            additional_docs: vec![],
        };

        Ok(Self::new(client, project_docs))
    }

    pub fn from_env() -> Self {
        let project_docs = ProjectDocsConfig {
            story_bible_id: std::env::var("PROJECT_DOC_STORY_BIBLE").ok(),
            script_id: std::env::var("PROJECT_DOC_SCRIPT").ok(),
            writers_notes_id: std::env::var("PROJECT_DOC_WRITERS_NOTES").ok(),
            additional_docs: vec![],
        };

        Self::new(GoogleDocsClient::new(), project_docs)
    }

    pub fn get_tools(&self, include_google_search: bool) -> Vec<AiTool> {
        let mut tools = Vec::new();

        if include_google_search {
            tools.push(AiTool::GoogleSearch);
        }

        tools.push(AiTool::FunctionDeclaration(read_google_doc_function()));

        if self.project_docs.story_bible_id.is_some()
            || self.project_docs.script_id.is_some()
            || self.project_docs.writers_notes_id.is_some()
            || !self.project_docs.additional_docs.is_empty()
        {
            tools.push(self.project_docs.as_function_tool());
        }

        tools
    }
}

#[async_trait]
impl FunctionCallHandler for GoogleDocsFunctionHandler {
    async fn handle_function_call(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        match name {
            "read_google_doc" => {
                let doc_id = args
                    .get("document_id")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'document_id' argument")?;

                let result = if self.client.auth.is_some() {
                    self.client.get_all_tabs_text(doc_id).await
                } else {
                    self.client.get_document_text(doc_id).await
                };

                match result {
                    Ok(content) => Ok(serde_json::json!({
                        "success": true,
                        "content": content,
                    })),
                    Err(e) => Ok(serde_json::json!({
                        "success": false,
                        "error": e.to_string(),
                    })),
                }
            }

            "read_project_doc" => {
                let doc_name = args
                    .get("document_name")
                    .and_then(|v| v.as_str())
                    .ok_or("Missing 'document_name' argument")?;

                let doc_id = self
                    .project_docs
                    .get_doc_id(doc_name)
                    .ok_or_else(|| format!("Unknown document: '{}'", doc_name))?;

                let result = if self.client.auth.is_some() {
                    self.client.get_all_tabs_text(doc_id).await
                } else {
                    self.client.get_document_text(doc_id).await
                };

                match result {
                    Ok(content) => Ok(serde_json::json!({
                        "success": true,
                        "document_name": doc_name,
                        "content": content,
                    })),
                    Err(e) => Ok(serde_json::json!({
                        "success": false,
                        "document_name": doc_name,
                        "error": e.to_string(),
                    })),
                }
            }

            _ => Err(format!("Unknown function: {}", name)),
        }
    }

    fn supported_functions(&self) -> Vec<String> {
        let mut functions = vec!["read_google_doc".to_string()];

        if self.project_docs.story_bible_id.is_some()
            || self.project_docs.script_id.is_some()
            || self.project_docs.writers_notes_id.is_some()
            || !self.project_docs.additional_docs.is_empty()
        {
            functions.push("read_project_doc".to_string());
        }

        functions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_doc_id_from_url() {
        let url = "https://docs.google.com/document/d/1abc123xyz/edit";
        assert_eq!(
            GoogleDocsClient::extract_doc_id(url),
            Some("1abc123xyz".to_string())
        );
    }

    #[test]
    fn test_extract_doc_id_from_id() {
        let id = "1abc123xyz";
        assert_eq!(
            GoogleDocsClient::extract_doc_id(id),
            Some("1abc123xyz".to_string())
        );
    }

    #[test]
    fn test_project_docs_config() {
        let config = ProjectDocsConfig {
            story_bible_id: Some("doc1".to_string()),
            script_id: Some("doc2".to_string()),
            writers_notes_id: None,
            additional_docs: vec![],
        };

        assert_eq!(config.get_doc_id("story_bible"), Some("doc1"));
        assert_eq!(config.get_doc_id("script"), Some("doc2"));
        assert_eq!(config.get_doc_id("writers_notes"), None);
    }
}
