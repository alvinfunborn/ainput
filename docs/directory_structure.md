### 参考目录结构

.
├── .git/
├── .specstory/
├── .cursor/
├── .vscode/
├── dist/
├── docs/
├── node_modules/
├── overlay.html
├── index.html
├── package.json
├── package-lock.json
├── README.md
├── README_zh.md
├── tailwind.config.js
├── tsconfig.json
├── tsconfig.tsbuildinfo
├── vite.config.ts
├── .gitignore
├── .cursorindexingignore
├── src/
│   ├── App.tsx
│   ├── main.tsx
│   ├── i18n.ts
│   ├── index.html
│   ├── locales/
│   │   ├── zh.json
│   │   └── en.json
│   ├── types/
│   │   └── config.ts
│   ├── styles/
│   │   ├── index.css
│   │   ├── global.css
│   │   └── App.css
│   ├── hooks/
│   │   └── useKeyOptions.ts
│   └── components/
│       └── settings/
│           ├── Settings.tsx
│           └── sections/
│               ├── KeybindingSettings.tsx
│               ├── UiAutomationSettings.tsx
│               ├── SystemSettings.tsx
│               ├── MouseSettings.tsx
│               ├── HintSettings.tsx
│               └── KeyboardSettings.tsx
├── src-tauri/
│   ├── Cargo.lock
│   ├── Cargo.toml
│   ├── Tauri.toml
│   ├── config.toml
│   ├── config.default
│   ├── build.rs
│   ├── logs/
│   ├── capabilities/
│   │   └── main.json
│   ├── icons/
│   │   └── icon.ico
│   ├── gen/
│   ├── target/
│   │   ├── .rustc_info.json
│   │   ├── CACHEDIR.TAG
│   │   ├── release/
│   │   │   ├── screen-buoy.zip
│   │   │   ├── screen-buoy.d
│   │   │   ├── screen_buoy.pdb
│   │   │   ├── screen-buoy.exe
│   │   │   ├── config.toml
│   │   │   ├── config.default
│   │   │   ├── .cargo-lock
│   │   │   ├── deps/
│   │   │   ├── .fingerprint/
│   │   │   │   └── ...（大量构建指纹目录和文件，略）
│   │   │   ├── build/
│   │   │   ├── examples/
│   │   │   ├── incremental/
│   │   └── debug/
│   └── src/
│       ├── main.rs
│       ├── lib.rs
│       ├── window/
│       │   ├── window.rs
│       │   └── mod.rs
│       ├── utils/
│       │   ├── logger.rs
│       │   ├── mod.rs
│       │   └── rect.rs
│       ├── input/
│       │   ├── mouse.rs
│       │   ├── hook.rs
│       │   ├── keyboard.rs
│       │   ├── executor.rs
│       │   └── mod.rs
│       ├── hint/
│       │   ├── overlay.rs
│       │   ├── mod.rs
│       │   ├── hint.rs
│       │   └── generator.rs
│       ├── element/
│       │   ├── ui_automation.rs
│       │   ├── element.rs
│       │   └── mod.rs
│       ├── config/
│       │   ├── mod.rs
│       │   ├── system.rs
│       │   ├── keyboard.rs
│       │   ├── keybinding.rs
│       │   ├── hint.rs
│       │   ├── ui_automation.rs
│       │   └── mouse.rs
│       └── monitor/
│           ├── monitor.rs
│           └── mod.rs