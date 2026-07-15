//! Microsoft Store 版 Codex 桌面应用的轻量只读安装检测。
//!
//! 目标是 MSIX 包身份 `OpenAI.Codex`（PackageFamilyName
//! `OpenAI.Codex_2p2nqsd0c76g0`，Store 产品 9PLM9XGG6VKS）。2026-07 起该包
//! 与 ChatGPT 桌面端合并品牌：显示名/主进程改为 "ChatGPT"，但包身份未变，
//! 因此只能匹配稳定的 package identity，绝不能依赖本地化显示名。同 publisher
//! 还存在独立的 `OpenAI.ChatGPT-Desktop_<同一hash>` 包，故也不能只看
//! publisher hash，必须匹配名称段 `OpenAI.Codex` 本身。
//!
//! 检测手段（任一命中即视为已安装；全部只读，不执行任何可执行文件——
//! `\Microsoft\WindowsApps` 下的 App Execution Alias 严禁执行，见 misc.rs）：
//! 1. HKCU AppModel Repository 包注册表：当前用户注册的 MSIX 包每个
//!    PackageFullName 一个子键（形如 `OpenAI.Codex_26.707.9981.0_x64__…`），
//!    无需管理员权限即可枚举。
//! 2. `%LOCALAPPDATA%\Packages\OpenAI.Codex_<hash>` 包数据目录存在。
//!
//! 桌面应用与 Codex CLI 官方共享同一个 `%USERPROFILE%\.codex`
//! （docs: developers.openai.com/codex/app/windows，本仓已实测验证；MSIX
//! 未虚拟化重定向该目录），因此检测到桌面应用后无需新增配置路径，现有
//! Codex 配置管理原样适用。该检测只是 CLI 探测之外的补充信号：失败一律
//! 返回 false（不会隐藏按钮——CLI 在装时仍显示），且绝不能让异常阻塞启动。

/// MSIX PackageFullName 形如 `Name_Version_Arch__PublisherHash`，
/// PackageFamilyName / 包数据目录名形如 `Name_PublisherHash`：名称段之后
/// 必有 `_`。据此用 `OpenAI.Codex_` 前缀（ASCII 大小写不敏感）同时匹配
/// 两种形态，并天然排除 `OpenAI.ChatGPT-Desktop_…` 与 `OpenAI.CodexFoo_…`。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn is_codex_desktop_package_name(name: &str) -> bool {
    const PREFIX: &str = "openai.codex_";
    // 用 get 而非切片：注册表键名/目录名可含多字节字符，硬切可能不在字符边界。
    name.get(..PREFIX.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(PREFIX))
}

/// 扫描包数据根目录（正常为 `%LOCALAPPDATA%\Packages`）下是否存在
/// OpenAI.Codex 的包目录。目录不可读/不存在一律 false。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn packages_dir_contains_codex_desktop(packages_dir: &std::path::Path) -> bool {
    let Ok(entries) = std::fs::read_dir(packages_dir) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(is_codex_desktop_package_name)
    })
}

/// HKCU\Software\Classes\Local Settings\...\AppModel\Repository\Packages 下
/// 枚举当前用户已注册的 MSIX 包全名并匹配。键不可读（策略限制等）一律 false。
/// 注：reg.exe `/f` 对该键搜索存在假阴性（本机实测），必须走 API 枚举。
#[cfg(target_os = "windows")]
fn registry_lists_codex_desktop() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const PACKAGES_KEY: &str = r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages";
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(packages) = hkcu.open_subkey(PACKAGES_KEY) else {
        return false;
    };
    packages
        .enum_keys()
        .flatten()
        .any(|key| is_codex_desktop_package_name(&key))
}

/// `%LOCALAPPDATA%` 解析：环境变量优先，缺失时退回 home\AppData\Local
/// （与 claude_desktop_config::windows_local_app_data_dir 同一模式，
/// get_home_dir 使 CC_SWITCH_TEST_HOME 隔离可用）。
#[cfg(target_os = "windows")]
fn windows_local_app_data_dir() -> std::path::PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::config::get_home_dir().join("AppData").join("Local"))
}

/// 轻量只读检测：Microsoft Store 版 Codex 桌面应用是否已为当前用户安装。
/// 非 Windows 平台恒为 false（该安装形态仅存在于 Windows）。
pub fn is_installed() -> bool {
    #[cfg(target_os = "windows")]
    {
        registry_lists_codex_desktop()
            || packages_dir_contains_codex_desktop(&windows_local_app_data_dir().join("Packages"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_name_matches_full_and_family_forms() {
        // PackageFullName（注册表键形态）
        assert!(is_codex_desktop_package_name(
            "OpenAI.Codex_26.707.9981.0_x64__2p2nqsd0c76g0"
        ));
        // PackageFamilyName（%LOCALAPPDATA%\Packages 目录形态）
        assert!(is_codex_desktop_package_name("OpenAI.Codex_2p2nqsd0c76g0"));
        // 注册表键名大小写不保证
        assert!(is_codex_desktop_package_name(
            "openai.codex_26.707.9981.0_X64__2P2NQSD0C76G0"
        ));
    }

    #[test]
    fn package_name_rejects_same_publisher_and_lookalikes() {
        // 同 publisher hash 的独立 ChatGPT 桌面包，绝不能命中
        assert!(!is_codex_desktop_package_name(
            "OpenAI.ChatGPT-Desktop_2p2nqsd0c76g0"
        ));
        // 名称段必须精确为 OpenAI.Codex（后随 `_`）
        assert!(!is_codex_desktop_package_name(
            "OpenAI.CodexFoo_1.0.0.0_x64__2p2nqsd0c76g0"
        ));
        assert!(!is_codex_desktop_package_name("OpenAI.Codex"));
        assert!(!is_codex_desktop_package_name(""));
        // 前缀长度处多字节字符：get(..len) 越界到非字符边界应安全返回 false
        assert!(!is_codex_desktop_package_name("OpenAI.Codex中"));
    }

    #[test]
    fn packages_dir_detection_by_directory_presence() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let packages = tmp.path().join("Packages");

        // 根目录不存在 → false（探测失败安全降级）
        assert!(!packages_dir_contains_codex_desktop(&packages));

        // 空目录 → false
        std::fs::create_dir_all(&packages).unwrap();
        assert!(!packages_dir_contains_codex_desktop(&packages));

        // 只有同 publisher 的 ChatGPT 桌面包 → false
        std::fs::create_dir(packages.join("OpenAI.ChatGPT-Desktop_2p2nqsd0c76g0")).unwrap();
        assert!(!packages_dir_contains_codex_desktop(&packages));

        // 出现 OpenAI.Codex 包目录 → true
        std::fs::create_dir(packages.join("OpenAI.Codex_2p2nqsd0c76g0")).unwrap();
        assert!(packages_dir_contains_codex_desktop(&packages));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn non_windows_is_never_installed() {
        assert!(!is_installed());
    }
}
