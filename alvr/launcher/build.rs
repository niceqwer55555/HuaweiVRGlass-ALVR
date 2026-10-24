#[cfg(windows)]
fn main() {
    let mut resource = winres::WindowsResource::new();
    // 沙箱/精简环境下注册表查询可能失败，显式提供 Windows SDK bin 目录
    let sdk_bin_candidates = [
        r"C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64",
        r"C:\Program Files (x86)\Windows Kits\10\bin\x64",
    ];
    for candidate in sdk_bin_candidates {
        if std::path::Path::new(candidate).join("rc.exe").exists() {
            resource.set_toolkit_path(candidate);
            break;
        }
    }
    resource.set_icon("../dashboard/resources/dashboard.ico");
    resource.compile().unwrap();
}

#[cfg(not(windows))]
fn main() {}
