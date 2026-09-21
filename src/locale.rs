use std::sync::{
    LazyLock,
    atomic::{AtomicU8, Ordering},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[repr(u8)]
pub enum Locale {
    #[serde(rename = "en")]
    English = 1,
    #[serde(rename = "zh-Hans")]
    Chinese = 2,
}

static SELECTED: AtomicU8 = AtomicU8::new(0);
static SYSTEM: LazyLock<Locale> = LazyLock::new(|| {
    let languages = crate::platform::locale::languages().unwrap_or_else(|error| {
        eprintln!("Preferred languages: {error}");
        Vec::new()
    });
    languages
        .iter()
        .find_map(|language| match language.split(['-', '_']).next() {
            Some("zh") => Some(Locale::Chinese),
            Some("en") => Some(Locale::English),
            _ => None,
        })
        .unwrap_or(Locale::English)
});

pub fn set(locale: Option<Locale>) {
    LazyLock::force(&SYSTEM);
    SELECTED.store(locale.map_or(0, |locale| locale as u8), Ordering::Relaxed);
}

pub fn current() -> Locale {
    match SELECTED.load(Ordering::Relaxed) {
        1 => Locale::English,
        2 => Locale::Chinese,
        _ => *SYSTEM,
    }
}

impl Locale {
    pub fn tag(self) -> &'static str {
        match self {
            Self::English => "en-US",
            Self::Chinese => "zh-CN",
        }
    }
}

macro_rules! tr {
    ($text:tt) => {
        match $crate::locale::current() {
            $crate::locale::Locale::English => $text,
            $crate::locale::Locale::Chinese => $crate::locale::zh!($text),
        }
    };
    ($text:tt, $($args:tt)+) => {
        match $crate::locale::current() {
            $crate::locale::Locale::English => format!($text, $($args)+),
            $crate::locale::Locale::Chinese => format!($crate::locale::zh!($text), $($args)+),
        }
    };
}

macro_rules! zh {
    ("Appearance") => {
        "外观"
    };
    ("Light") => {
        "浅色"
    };
    ("Dark") => {
        "深色"
    };
    ("Configurations") => {
        "配置"
    };
    ("Configuration") => {
        "配置"
    };
    ("New configuration") => {
        "新建配置"
    };
    ("Create configuration") => {
        "创建配置"
    };
    ("Delete configuration") => {
        "删除配置"
    };
    ("Name") => {
        "名称"
    };
    ("Run when switching to") => {
        "切换为以下外观时运行"
    };
    ("Run") => {
        "运行"
    };
    ("Action") => {
        "动作"
    };
    ("Actions") => {
        "动作"
    };
    ("Add action") => {
        "添加动作"
    };
    ("System appearance") => {
        "系统外观"
    };
    ("Wallpaper") => {
        "壁纸"
    };
    ("Command") => {
        "命令"
    };
    ("Windows theme") => {
        "Windows 主题"
    };
    ("File") => {
        "文件"
    };
    ("Choose…") => {
        "选择…"
    };
    ("Program") => {
        "程序"
    };
    ("Arguments") => {
        "参数"
    };
    ("Argument {number}") => {
        "参数 {number}"
    };
    ("Add argument") => {
        "添加参数"
    };
    ("Continue after the program exits") => {
        "程序结束后继续"
    };
    ("Wallpaper preview unavailable") => {
        "无法显示壁纸预览"
    };
    ("Drop a file or enter its path") => {
        "拖入文件或输入路径"
    };
    ("Use this file") => {
        "使用此文件"
    };
    ("Save") => {
        "保存"
    };
    ("Dismiss") => {
        "关闭提示"
    };
    ("Cancel") => {
        "取消"
    };
    ("Edit") => {
        "编辑"
    };
    ("Edit…") => {
        "编辑…"
    };
    ("Remove") => {
        "移除"
    };
    ("Clear") => {
        "清除"
    };
    ("Schedule") => {
        "计划"
    };
    ("Automatic switching") => {
        "自动切换"
    };
    ("New rule") => {
        "新建计划"
    };
    ("Add rule") => {
        "添加计划"
    };
    ("Save rule") => {
        "保存计划"
    };
    ("Delete rule") => {
        "删除计划"
    };
    ("Add arrangement") => {
        "添加计划"
    };
    ("Arrangement") => {
        "计划"
    };
    ("Days") => {
        "重复日期"
    };
    ("Time") => {
        "时间"
    };
    ("Choose another time") => {
        "选择其他时间"
    };
    ("Apply schedule on launch") => {
        "启动时按计划切换"
    };
    ("Every day") => {
        "每天"
    };
    ("Weekdays") => {
        "工作日"
    };
    ("Weekends") => {
        "周末"
    };
    ("{days}, {mode}") => {
        "{days}，{mode}"
    };
    ("{weekday} {time}") => {
        "{weekday}{time}"
    };
    ("{title}: {target}") => {
        "{title}：{target}"
    };
    ("Switch to {mode} at {time}") => {
        "{time}切换为{mode}"
    };
    ("{days} at {time}, switch to {mode}") => {
        "{days}{time}切换为{mode}"
    };
    ("Settings") => {
        "设置"
    };
    ("Settings…") => {
        "设置…"
    };
    ("Language") => {
        "语言"
    };
    ("System default") => {
        "跟随系统"
    };
    ("Launch at login") => {
        "登录时启动"
    };
    ("Global shortcut") => {
        "全局快捷键"
    };
    ("Record global shortcut") => {
        "录入全局快捷键"
    };
    ("Record shortcut") => {
        "录入快捷键"
    };
    ("Record shortcut…") => {
        "录入快捷键…"
    };
    ("Press a shortcut…") => {
        "按下快捷键…"
    };
    ("Press a key combination") => {
        "按下组合键"
    };
    ("Cancel recording") => {
        "取消录入"
    };
    ("Remove shortcut") => {
        "移除快捷键"
    };
    ("None") => {
        "无"
    };
    ("Show pola") => {
        "显示 pola"
    };
    ("Quit pola") => {
        "退出 pola"
    };
    ("Exit") => {
        "退出"
    };
    ("Undo") => {
        "撤销"
    };
    ("Cut") => {
        "剪切"
    };
    ("Copy") => {
        "复制"
    };
    ("Paste") => {
        "粘贴"
    };
    ("Select All") => {
        "全选"
    };
    ("Switch between light and dark appearance from any application.") => {
        "在任意应用中切换系统浅色与深色。"
    };
    ("Manual changes take effect immediately. Future scheduled changes continue normally.") => {
        "手动切换立即生效，之后的计划仍按时执行。"
    };
    ("Enter a configuration name.") => {
        "请输入配置名称。"
    };
    ("A configuration named {name:?} already exists.") => {
        "已存在名为{name:?}的配置。"
    };
    ("Configuration {name:?} was not found.") => {
        "找不到名为{name:?}的配置。"
    };
    ("A configuration is already running.") => {
        "正在执行配置。"
    };
    ("This configuration has been removed.") => {
        "此配置已被删除。"
    };
    ("This rule has been removed.") => {
        "此计划已被删除。"
    };
    ("This action has been removed.") => {
        "此动作已被删除。"
    };
    ("The configuration editor has closed.") => {
        "配置编辑器已关闭。"
    };
    ("Enter a program to run.") => {
        "请输入要运行的程序。"
    };
    ("Enter a wallpaper path.") => {
        "请输入壁纸路径。"
    };
    ("Enter a theme path.") => {
        "请输入主题路径。"
    };
    ("Drop a file from File Explorer.") => {
        "请从文件资源管理器拖入文件。"
    };
    ("Drop one file at a time.") => {
        "每次请拖入一个文件。"
    };
    ("The dropped item must be a file.") => {
        "拖入的项目需要是文件。"
    };
    ("Choose a time.") => {
        "请选择时间。"
    };
    ("Choose at least one day.") => {
        "请至少选择一天。"
    };
    ("Could not focus the input.") => {
        "无法激活输入框。"
    };
    ("Could not focus the shortcut recorder.") => {
        "无法激活快捷键录入。"
    };
    ("This key cannot be used as a global shortcut.") => {
        "此按键无法用作全局快捷键。"
    };
    ("Could not restore shortcut") => {
        "无法恢复快捷键"
    };
    ("Could not restore the shortcut: {error}") => {
        "无法恢复快捷键：{error}"
    };
    ("{error}\nShortcut: {restore}") => {
        "{error}\n快捷键：{restore}"
    };
    ("Could not activate the window.") => {
        "无法激活窗口。"
    };
    ("Could not open the file picker.") => {
        "无法打开文件选择器。"
    };
    ("Could not open window") => {
        "无法打开窗口"
    };
    ("Could not exit pola") => {
        "无法退出 pola"
    };
    ("{error}\nLaunch at login: {restore}") => {
        "{error}\n登录启动：{restore}"
    };
    ("{variable} is not set") => {
        "未设置 {variable} 环境变量"
    };
    ("Could not change appearance") => {
        "无法切换外观"
    };
    ("Could not start pola") => {
        "无法启动 pola"
    };
    ("Could not create appearance script") => {
        "无法创建外观切换脚本"
    };
    ("Login item must contain a property list dictionary") => {
        "登录启动文件需要包含属性列表字典"
    };
    ("Login item path is occupied by another service") => {
        "登录启动文件的位置已被其他服务使用"
    };
    ("Connection to pola background process closed") => {
        "与 pola 后台进程的连接已关闭"
    };
    ("daemon socket path is occupied by another file") => {
        "后台通信路径已被其他文件使用"
    };
    ("daemon exited before opening its connection") => {
        "后台进程在建立连接前退出"
    };
    ("Configuration name must be Unicode.") => {
        "配置名称需要使用 Unicode 字符。"
    };
    ("Usage: pola [daemon | run NAME]") => {
        "用法：pola [daemon | run 名称]"
    };
}

pub(crate) use {tr, zh};

pub fn days(days: &[crate::schedule::Weekday]) -> Result<String, String> {
    use crate::schedule::Weekday;
    let mask = days.iter().fold(0u8, |mask, day| mask | (1 << *day as u8));
    match mask {
        0b1111111 => return Ok(tr!("Every day").into()),
        0b0011111 => return Ok(tr!("Weekdays").into()),
        0b1100000 => return Ok(tr!("Weekends").into()),
        _ => {}
    }
    let names = crate::platform::locale::weekdays()?;
    let selected: Vec<_> = Weekday::ALL
        .iter()
        .enumerate()
        .filter_map(|(index, day)| days.contains(day).then_some(names[index].as_str()))
        .collect();
    Ok(selected.join(match current() {
        Locale::English => ", ",
        Locale::Chinese => "、",
    }))
}

pub fn next(event: &crate::schedule::Event) -> Result<String, String> {
    let names = crate::platform::locale::weekdays()?;
    let weekday = &names[crate::schedule::Weekday::from(event.at.date().weekday()) as usize];
    let time = tr!(
        "{weekday} {time}",
        weekday = weekday,
        time = crate::platform::locale::time(event.at.time())?
    );
    Ok(tr!(
        "Switch to {mode} at {time}",
        mode = event.mode.label(),
        time = time
    ))
}
