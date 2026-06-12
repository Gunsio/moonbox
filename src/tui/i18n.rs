use crate::core::config::UiLanguage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Text {
    SettingsTitle,
    SettingsSubtitle,
    Language,
    Theme,
    HooksEventChannel,
    HooksManagedByCli,
    SmartEnterTmux,
    CurrentEnterRoute,
    Effect,
    SettingsSafety,
    SettingsKeys,
    Unsaved,
    Saved,
    Enabled,
    Disabled,
    Draft,
    NoSelectedSession,
    GeneratingReview,
    Session,
    Target,
    Result,
    Source,
    Command,
    Action,
    Label,
    RewindPoint,
    Validation,
    TargetCommand,
    TargetReceives,
    Readiness,
    TargetContent,
    Goal,
    State,
    Decision,
    Todo,
    Risk,
    Program,
    Directory,
    Arguments,
    Prompt,
    ArgumentCountHandoff,
    PromptDisplayedBelow,
    Rerun,
    CopyCommand,
    Back,
    CannotRun,
    CannotCopy,
    DraftCannotRun,
    RunLocalTarget,
    ValidationFailed,
    ChooseAiSkillToRun,
    ScrollOnlyKeys,
    ReviewActionKeys,
    LoadingHiddenJob,
    WaitBackground,
    NextCopyOnly,
    NextRunCopy,
    LocalSourceOriginal,
    SshSourceReadOnly,
    TargetLaunchNoAuto,
}

pub fn text(language: UiLanguage, key: Text) -> &'static str {
    match language {
        UiLanguage::English => english(key),
        UiLanguage::ZhHans => zh_hans(key),
    }
}

pub fn on_off(language: UiLanguage, value: bool) -> &'static str {
    match (language, value) {
        (UiLanguage::English, true) => "On",
        (UiLanguage::English, false) => "Off",
        (UiLanguage::ZhHans, true) => "开",
        (UiLanguage::ZhHans, false) => "关",
    }
}

pub fn language_name(language: UiLanguage, value: UiLanguage) -> &'static str {
    match language {
        UiLanguage::English => value.label(),
        UiLanguage::ZhHans => match value {
            UiLanguage::English => "English",
            UiLanguage::ZhHans => "简体中文",
        },
    }
}

fn english(key: Text) -> &'static str {
    match key {
        Text::SettingsTitle => "Settings",
        Text::SettingsSubtitle => {
            "Preview first. Saved settings apply to Moonbox UI only, not source session stores."
        }
        Text::Language => "Language",
        Text::Theme => "Theme",
        Text::HooksEventChannel => "Hooks event channel",
        Text::HooksManagedByCli => "install/uninstall is managed by `moonbox hooks`",
        Text::SmartEnterTmux => "Smart Enter / tmux",
        Text::CurrentEnterRoute => "Current Enter Route",
        Text::Effect => "Effect",
        Text::SettingsSafety => {
            "Moonbox never translates session content, creates panes, sends keystrokes, resumes source sessions, or mutates source stores from these settings."
        }
        Text::SettingsKeys => {
            "j/k choose   h/l change   space toggle   r reset   Enter save   Esc cancel"
        }
        Text::Unsaved => "Unsaved",
        Text::Saved => "Saved",
        Text::Enabled => "Enabled",
        Text::Disabled => "Disabled",
        Text::Draft => "Draft",
        Text::NoSelectedSession => "No selected session",
        Text::GeneratingReview => "Generating Handoff Review",
        Text::Session => "Session",
        Text::Target => "Target",
        Text::Result => "Result",
        Text::Source => "Source",
        Text::Command => "Command",
        Text::Action => "Action",
        Text::Label => "Label",
        Text::RewindPoint => "Rewind",
        Text::Validation => "Validation",
        Text::TargetCommand => "Target command",
        Text::TargetReceives => "Target receives",
        Text::Readiness => "Readiness",
        Text::TargetContent => "Content sent to target",
        Text::Goal => "Goal",
        Text::State => "State",
        Text::Decision => "Decision",
        Text::Todo => "Todo",
        Text::Risk => "Risk",
        Text::Program => "Program",
        Text::Directory => "Directory",
        Text::Arguments => "Arguments",
        Text::Prompt => "Prompt",
        Text::ArgumentCountHandoff => "arguments; the last one is the handoff prompt",
        Text::PromptDisplayedBelow => "shown in full below as the final argument",
        Text::Rerun => "Run again",
        Text::CopyCommand => "Copy command",
        Text::Back => "Back",
        Text::CannotRun => "Cannot run",
        Text::CannotCopy => "Cannot copy",
        Text::DraftCannotRun => "Draft cannot run",
        Text::RunLocalTarget => "Run local target",
        Text::ValidationFailed => "validation failed",
        Text::ChooseAiSkillToRun => "choose an AI skill to run",
        Text::ScrollOnlyKeys => "gg top   G bottom   j/k scroll",
        Text::ReviewActionKeys => "gg top   G bottom   j/k scroll   r/y/Esc actions",
        Text::LoadingHiddenJob => {
            "Esc hides this panel; the handoff job continues in the background."
        }
        Text::WaitBackground => "background job",
        Text::NextCopyOnly => "Next: press y to copy the command; Esc returns",
        Text::NextRunCopy => {
            "Next: press r to start the local target; press y to copy; Esc returns"
        }
        Text::LocalSourceOriginal => "Local source: original resume is still available with o.",
        Text::SshSourceReadOnly => {
            "SSH source is read-only: Moonbox builds a local target handoff, not remote resume."
        }
        Text::TargetLaunchNoAuto => {
            "Nothing opens or resumes automatically; choose the next action explicitly."
        }
    }
}

fn zh_hans(key: Text) -> &'static str {
    match key {
        Text::SettingsTitle => "设置",
        Text::SettingsSubtitle => {
            "先预览再保存。设置只影响 Moonbox UI，不影响 source session store。"
        }
        Text::Language => "语言",
        Text::Theme => "主题",
        Text::HooksEventChannel => "Hooks 事件通道",
        Text::HooksManagedByCli => "安装/卸载由 `moonbox hooks` 管理",
        Text::SmartEnterTmux => "Smart Enter / tmux",
        Text::CurrentEnterRoute => "当前 Enter 路径",
        Text::Effect => "影响",
        Text::SettingsSafety => {
            "这些设置不会翻译 session 内容、创建 pane、发送按键、恢复 source session 或修改 source store。"
        }
        Text::SettingsKeys => "j/k 选择   h/l 切换   space 开关   r 重置   Enter 保存   Esc 取消",
        Text::Unsaved => "未保存",
        Text::Saved => "已保存",
        Text::Enabled => "已启用",
        Text::Disabled => "未启用",
        Text::Draft => "草稿",
        Text::NoSelectedSession => "未选择 session",
        Text::GeneratingReview => "正在生成 Handoff Review",
        Text::Session => "会话",
        Text::Target => "目标",
        Text::Result => "结果",
        Text::Source => "来源",
        Text::Command => "命令",
        Text::Action => "动作",
        Text::Label => "标签",
        Text::RewindPoint => "回退点",
        Text::Validation => "校验",
        Text::TargetCommand => "目标命令",
        Text::TargetReceives => "目标会收到",
        Text::Readiness => "就绪检查",
        Text::TargetContent => "传给目标的内容",
        Text::Goal => "目标",
        Text::State => "状态",
        Text::Decision => "决策",
        Text::Todo => "待办",
        Text::Risk => "风险",
        Text::Program => "程序",
        Text::Directory => "目录",
        Text::Arguments => "参数",
        Text::Prompt => "Prompt",
        Text::ArgumentCountHandoff => "个参数，最后一个是 handoff prompt",
        Text::PromptDisplayedBelow => "下面完整展示，会作为最后一个参数传入",
        Text::Rerun => "再运行",
        Text::CopyCommand => "复制命令",
        Text::Back => "返回",
        Text::CannotRun => "不可运行",
        Text::CannotCopy => "不可复制",
        Text::DraftCannotRun => "草稿不可运行",
        Text::RunLocalTarget => "运行本地目标",
        Text::ValidationFailed => "校验未通过",
        Text::ChooseAiSkillToRun => "选择 AI skill 后可运行",
        Text::ScrollOnlyKeys => "gg 顶部   G 底部   j/k 滚动",
        Text::ReviewActionKeys => "gg 顶部   G 底部   j/k 滚动   r/y/Esc 操作",
        Text::LoadingHiddenJob => "Esc 隐藏此面板；handoff 任务会继续在后台运行。",
        Text::WaitBackground => "后台生成",
        Text::NextCopyOnly => "下一步: 当前只能按 y 复制命令；Esc 返回",
        Text::NextRunCopy => "下一步: 按 r 启动本地目标；按 y 复制命令；Esc 返回",
        Text::LocalSourceOriginal => "本地 source：原生 resume 仍可用 o 单独打开。",
        Text::SshSourceReadOnly => {
            "SSH source 只读：Moonbox 只构建本地目标 handoff，不远程 resume。"
        }
        Text::TargetLaunchNoAuto => "下一步不会自动打开或恢复 session；需要你明确选择。",
    }
}
