# ainput

> AI 驱动的全局输入法，智能候选，极致本地化  
> ⚠️ 本项目涉及输入内容、窗口信息等数据上传至大模型服务商，详见下文"隐私声明"

---

## 需求背景

典型场景：
- 在任意输入框输入拼音或任意文本，自动弹出 AI 智能候选
- 多屏/高DPI 环境下，候选框总能精准跟随输入框
- 输入历史、剪贴板内容智能融合，提升输入效率

---

## 使用示例

- **AI 智能候选**  

---

## 实现原理

ainput 采用 Tauri 全栈架构，后端用 Rust，前端用 React，核心流程如下：

- 后端通过 Windows UI Automation 获取当前聚焦窗口和输入框信息
- 监听输入焦点和内容变化，自动弹出/隐藏悬浮候选框（overlay）
- overlay 用 Tauri Webview 实现，前端 React 渲染
- AI 候选通过本地/远程 API 获取，支持 mock 和自定义 key
- 输入历史和剪贴板内容用 SQLite 管理，智能融合
- 配置用 TOML，支持自定义历史 TTL、刷新频率、快捷键等
- 日志细致，便于排查和追踪
- 部分数据会上传到大模型服务商，详见"隐私声明"

---

## 安装与使用

##### 方式一：直接下载

1. 前往 [Releases 页面](https://github.com/alvinfunborn/ainput/releases) 下载最新的 `ainput.exe` 和 `config.toml` 文件。
2. 将二者放在同一目录下，修改config.toml添加大模型服务商，双击运行 `ainput.exe`。
3. 托盘会出现 ainput 图标，可右键设置。
4. 如需自定义配置，编辑 `config.toml`，保存后重启生效。

##### 方式二：源码编译运行

```bash
# 克隆仓库
git clone https://github.com/alvinfunborn/ainput.git
cd ainput

# 安装依赖
npm install

# 构建 Tauri 后端
cd src-tauri
cargo build

# 开发模式启动
cd ..
npm run tauri dev
```

- 配置详见 `src-tauri/config.toml`
- 支持托盘、开机自启、快捷键自定义

---

## 默认快捷键

- 候选框激活时
  - Tab：选择候选词
  - 1: 选择候选词的第一个字
  - Esc：关闭候选框
- 其它快捷键可在配置中自定义

---

## 性能

- 内存占用低，常驻后台无压力
- CPU 占用极低，支持多屏/高DPI
- 启动速度快，日志丰富

---

## 安全性

- 开源可审计，无后门
- 仅需普通用户权限

### 隐私声明

ainput 在生成 AI 候选词时，会采集并上传如下信息到大模型服务商：
- 当前输入框的内容（你正在输入的文本）
- 当前窗口的应用名、标题、类名、坐标等
- 输入历史（部分内容）
- 剪贴板内容（部分场景）

这些信息会被拼接为 prompt/context，发送到远程 AI 服务，用于生成候选词。

**隐私保护现状**：
- 支持通过脱敏正则对部分敏感内容进行脱敏处理
- 可通过 ignore_apps 配置忽略指定应用，不采集其数据
- 但仍可能存在未覆盖的隐私风险，部分敏感信息可能被上传
- 目前尚未实现本地加密或更细粒度的隐私过滤

**用户须知**：
- 使用前请充分了解：你的输入内容、窗口信息、历史、剪贴板等可能会被上传到大模型服务商
- 这些服务商的隐私政策请自行查阅
- 若对隐私有高要求，请勿在敏感场景下使用，或关闭 AI 功能
- 本项目不对因隐私泄露造成的后果负责

---

## 附录

- [Windows Virtual Key Codes](https://docs.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes)
- [Windows UI Automation](https://learn.microsoft.com/en-us/windows/win32/winauto/uiauto-controltype-ids)
