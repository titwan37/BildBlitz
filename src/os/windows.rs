#[allow(dead_code)]
pub fn apply_mica(_window: &eframe::Frame) {
    // Windows 11 Mica effect implementation would go here
}

/// Context menu extension helper for Windows Explorer registration.
#[allow(dead_code)]
pub fn register_explorer_context_menu() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe_str = exe_path.to_str().ok_or("Invalid EXE path")?;

        let cmd = format!("\"{}\" --smart-folder \"%1\"", exe_str);

        let output = Command::new("reg")
            .args(&[
                "add",
                "HKCU\\Software\\Classes\\SystemFileAssociations\\image\\shell\\BildBlitzSmartFolder",
                "/ve",
                "/d",
                "Move to New Folder with Smart Naming",
                "/f",
            ])
            .output();

        let _ = Command::new("reg")
            .args(&[
                "add",
                "HKCU\\Software\\Classes\\SystemFileAssociations\\image\\shell\\BildBlitzSmartFolder\\command",
                "/ve",
                "/d",
                &cmd,
                "/f",
            ])
            .output();

        if let Ok(out) = output {
            if out.status.success() {
                return Ok(());
            }
        }
        Err("Failed to execute registry update".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(())
    }
}
