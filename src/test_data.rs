use tower_lsp::lsp_types::*;

pub const RAW_MAGIT_DIFF_RUST: &str = r#"Project: magit: diff-lsp
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

pub const RAW_MAGIT_DIFF_GO: &str = r#"Project: magit: lsp-example
Root: /Users/chrishipple/lsp-example/
Buffer: lsp-example
Type: magit-status
Head:     main little cleanup
Merge:    origin/main little cleanup
Push:     origin/main little cleanup

Unstaged changes (2)
modified   main.go
@@ -11,9 +11,10 @@ import (
    "github.com/TobiasYin/go-lsp/logs"
 )

+var logger *log.Logger
+var logPath *string
+
 func init() {
-	var logger *log.Logger
-	var logPath *string
    defer func() {
        logs.Init(logger)
    }()
@@ -34,7 +35,8 @@ func init() {
    }
    f, err := os.Create(p)
    if err == nil {
-		logger = log.New(f, "", 0)
+		logger = log.New(f, "new", 0)
+		logger.Println("My Logging start")
        return
    }
    panic(fmt.Sprintf("logs init error: %v", err))
@@ -42,6 +44,8 @@ func init() {
 }

 func main() {
+	logger.Println("Logging start")
+	fmt.Printf("log.Logger: %v\n", "starting")
    lsp_server := server.MyServer()
    lsp_server.Run()
 }
modified   server/server.go
@@ -9,14 +9,14 @@ import (

 func MyServer() *lsp.Server {
    progress := false
-	options := &lsp.Options{
+	options := lsp.Options{
        Address: "127.0.0.1:9907",
        HoverProvider: &defines.HoverOptions{
            WorkDoneProgressOptions: defines.WorkDoneProgressOptions{
                WorkDoneProgress: &progress}},
+		TextDocumentSync: 0,
    }
-	server := lsp.NewServer(options)
+	server := lsp.NewServer(&options)
    server.OnHover(components.Hover)
-
    return server
 }

Recent commits
1332ff8 origin/main little cleanup
1511d2c a bit more hover stuff
3d20bcb working hover provider!
401e3c0 doing golang
c800636 Initial commit
"#;

pub fn get_init_params() -> tower_lsp::lsp_types::InitializeParams {
    #[allow(deprecated)] // root_path is deprecated but without it, code doesn't compile? :(
    InitializeParams {
        process_id: None,
        root_path: None,
        root_uri: None,
        initialization_options: None,
        capabilities: ClientCapabilities {
            workspace: None,
            text_document: {
                Some(TextDocumentClientCapabilities {
                    hover: Some(HoverClientCapabilities::default()),
                    references: Some(ReferenceClientCapabilities {
                        dynamic_registration: Some(false),
                    }),
                    ..Default::default()
                })
            },
            window: None,
            general: None,
            experimental: None,
        },
        trace: None,
        workspace_folders: None,
        client_info: None,
        locale: None,
    }
}

pub fn get_open_params_rust(uri: Url) -> tower_lsp::lsp_types::DidOpenTextDocumentParams {
    DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: (uri),
            language_id: "rust".to_string(),
            version: 1,
            text: RAW_MAGIT_DIFF_RUST.to_string(),
        },
    }
}

pub fn get_open_params_go(uri: Url) -> tower_lsp::lsp_types::DidOpenTextDocumentParams {
    DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: (uri),
            language_id: "go".to_string(),
            version: 1,
            text: RAW_MAGIT_DIFF_GO.to_string(),
        },
    }
}
