#![allow(non_camel_case_types, non_snake_case)]
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    match sm_todo::application::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            report_startup_error(&error.to_string());
            std::process::ExitCode::FAILURE
        }
    }
}

fn report_startup_error(message: &str) {
    #[cfg(target_os = "windows")]
    {
        use windows::{
            core::PCWSTR,
            Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK},
        };
        let text: Vec<u16> = format!("数据库加载失败：{message}")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let title: Vec<u16> = "sm_todo".encode_utf16().chain(Some(0)).collect();
        unsafe {
            MessageBoxW(
                None,
                PCWSTR(text.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }
    #[cfg(not(target_os = "windows"))]
    eprintln!("数据库加载失败：{message}");
}
