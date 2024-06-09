pub const RAW_MAGIT_DIFF: &str = r#"Project: magit: diff-lsp
Root: /Users/chrishipple/diff-lsp/
Buffer: diff-lsp
Type: magit-status
Head:     main readme typo
Merge:    origin/main readme typo
Push:     origin/main readme typo

Unstaged changes (1)
modified   src/client.rs
@@ -60,9 +60,10 @@ impl LspClient {
                 text_document: {
                     Some(TextDocumentClientCapabilities {
                         hover: Some(HoverClientCapabilities::default()),
-                        ..Default::default()
-                    })
-                },
+                        references: Some(ReferenceClientCapabilities{
+                            include_declaration: true
+                        }),
+                        ..Default::default()},
                 window: None,
                 general: None,
                 experimental: None,
@@ -72,17 +73,17 @@ impl LspClient {
             client_info: None,
             locale: None,
         };
-        let message = "initialize".to_string();
+        let message_type = "initialize".to_string();  // TODO: Is there an enum for this?

-        let raw_resp = self.send_request(message, params).unwrap();
+        let raw_resp = self.send_request(message_type, params).unwrap();
         let resp: InitializeResult = serde_json::from_value(raw_resp).unwrap();
         println!("We got the response: {resp:?}");

         return Ok(resp);
     }

-    pub fn send_request<P: Serialize>(&mut self, message: String, params: P) -> Result<Value> {
-        if message == "initialize".to_string() {
+    pub fn send_request<P: Serialize>(&mut self, message_type: String, params: P) -> Result<Value> {
+        if message_type == "initialize".to_string() {
             let _ser_params = serde_json::to_value(params).unwrap();
             let raw_resp = self.send_value_request(_ser_params).unwrap();
             let as_value: Value = serde_json::from_str(&raw_resp).unwrap();

Recent commits
97f1e20 origin/main readme typo
f3b9f94 send message and serialize response (init message atleast)
803d9f2 send message with full body, work on parse resposne
6edde96 MVP--We can send the stdin message to server; working on format & reading response
f3cad47 MVP of starting the server and reading the stdout
d083654 more readme
577afab Create rust.yml
9ce2121 working on adding the client
8ffb4ce Added hover support with static suggestion
4d7867a following tutorial on tower-lsp

"#;
