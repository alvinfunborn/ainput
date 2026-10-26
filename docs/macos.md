# macOS

AInput follows Screen Buoy's platform separation: Accessibility for focused
controls, a Core Graphics event tap on a dedicated run loop, and an accessory
app with a passive, click-through overlay. Windows retains UIA and Win32.

## Build and launch

```sh
npm ci
rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm run tauri -- build --target universal-apple-darwin --bundles app
open src-tauri/target/universal-apple-darwin/release/bundle/macos/ainput.app
```

For just the current Mac, omit `--target universal-apple-darwin`. Copy the app to
Applications for a stable installed path. Release archives are ad-hoc signed,
not notarized. For repeated local builds, use the same valid development
certificate (`security find-identity -v -p codesigning`) before installing:

```sh
# Set AINPUT_SIGNING_IDENTITY to the intended valid certificate name or SHA-1.
codesign --force --deep --sign "$AINPUT_SIGNING_IDENTITY" \
  src-tauri/target/universal-apple-darwin/release/bundle/macos/ainput.app
codesign --verify --deep --strict \
  src-tauri/target/universal-apple-darwin/release/bundle/macos/ainput.app
```

Ad-hoc signatures change identity with the binary; a fixed path alone does not
preserve Accessibility authorization across those updates. Moving from ad-hoc
to a development certificate also requires authorizing that new identity once.

The menu bar's Settings action opens provider configuration and permission
guidance. Enable **Accessibility** for that exact app. A working keyboard tap
plus Accessibility is the ready condition. Input Monitoring appears only as
troubleshooting when keyboard capture is unavailable. The tap retries after
permission changes; restart if necessary. Replacing the app can require
removing and re-adding its Accessibility entry.

Startup checks permissions before choosing the settings page. Missing
Accessibility opens **权限与隐私** immediately; if Accessibility is allowed,
the keyboard tap has up to two seconds to initialize before showing recovery
guidance. Permission status stays visible across tabs and refreshes automatically
when returning from System Settings. The primary action opens the appropriate
system settings pane; the user enables the permission there.

Settings and the macOS menu bar use the same centered A/caret artwork. The app
icon uses Apple's rounded silhouette and standard transparent padding. To
regenerate the checked-in assets with Xcode Icon Composer, run
`bash scripts/generate-macos-icons.sh`.

Configure an API key or local command before using suggestions. An unconfigured
API provider does not send requests on macOS. Tab accepts, Escape dismisses,
and 1 accepts the first character. Command, Option, Control and Shift
combinations pass through normally. For non-ASCII input methods, suggestions
wait until a Space/Enter commit, Escape, or click after composition begins;
Tab and numeric selection keys remain available to the input method.

For a local CLI, use its absolute executable path if it is not found when
launching from Finder; GUI apps may have a different `PATH` from your terminal.
`{{prompt}}` passes the prompt as an argument, `{{prompt_file}}` supplies a
temporary file, and a command with neither placeholder receives stdin.

## Gemini API and proxies

Use provider `API`, model `gemini-3.5-flash-lite`, and URL
`https://generativelanguage.googleapis.com/v1beta/openai/chat/completions`.
Enter your Google AI Studio API key in the masked API Key field. The connection
test sends only fixed sample text and reports time to first text and total time.
Settings now groups connection, input shortcuts, prompt, and permissions/privacy
in a resizable sidebar layout. Changes require **保存更改**; **保存并测试** saves
the current configuration before sending the sample. A failed file write is
reported and does not change the running configuration. Permissions are under
**权限与隐私**. App exclusions require a restart after saving.

Google may return a JSON array error for streaming requests. AInput preserves
that error's status/message and explains region, credential, quota, model, and
network errors separately. `User location is not supported for the API use`
means Google's region check rejected the current network exit; it does not
establish whether the API key is valid. Change the system proxy exit and retry.

API requests follow `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY`, falling back to
macOS system HTTP(S) proxies when the environment has none. This works when
launching from Finder. The optional API proxy field overrides automatic
selection, e.g. `http://127.0.0.1:7890`. Connections are pooled; automatic proxy settings are rechecked every 30 seconds
on the next request. An unchanged configuration keeps the pool, avoiding periodic
TLS handshakes during typing. Idle connections expire normally. PAC scripts are
not supported. Switching a node inside the proxy app can leave existing tunnels
on that node until they close; use the proxy app's close-connections action after
a node switch if necessary.
CLI commands use their own networking configuration; the API proxy field does
not change a CLI's authentication or proxy behavior.

Official Gemini HTTPS requests use Rustls with HTTP/2 negotiation; custom
endpoints retain native TLS and system certificate trust.

API logs record HTTP status/protocol, time to first text, and total duration, without
logging prompts or API keys. SSE events are buffered across network chunks so
fragmented JSON and Chinese/emoji text are preserved.

## Data and compatibility

- Configuration: `~/Library/Application Support/com.ainput.dev/config.toml`.
  The bundled `config_macos.toml` seeds it once; upgrades preserve it.
- History: `input.db` in the same directory; logs are in its `logs/` directory.
- `AINPUT_DATA_DIR` overrides that directory for isolated development/tests.
- Supported controls expose AXTextField, AXTextArea or AXComboBox, a text value,
  and a collapsed selection. Password/protected fields and selected text are
  excluded. Custom-drawn controls without an editable AX element are skipped.
- Completion context ends at the caret. Insertion preserves subsequent text
  and handles UTF-16 offsets, including emoji. The worker revalidates the app,
  window, control, text and selection before writing.
- Writing uses AXSelectedText when writable. Otherwise Unicode events target
  the original PID without replacing the clipboard, in chunks of at most 20
  UTF-16 units with cleared modifiers and intact surrogate pairs. Each prefix
  is verified before sending the next; focus/caret changes stop the remainder.
  An unconfirmed/failed write is never automatically retried: the overlay shows
  an error and stops consuming acceptance keys until new input. Logs record the
  route, lengths and verification time, never the inserted text.
- The overlay uses global logical screen points and does not activate the app.
  AX objects remain on one worker; AppKit window/clipboard work runs on the main
  thread. Clipboard context is limited to 1,024 Unicode characters of plain text.

## Validation

```sh
npm run build
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib
```

For desktop acceptance, grant Accessibility, configure a provider, and check
TextEdit plus a browser input: accept a completion; move the caret into the
middle of text; switch windows before accepting; test Command+Tab, Shift+Tab,
Chinese input and emoji; verify password fields produce no suggestions. Check
Retina/external-monitor placement and full-screen apps. Build/test success
alone does not establish those desktop behaviors.
