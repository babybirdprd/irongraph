
pub enum ShellType {
    Bash,
    Cmd,
    PowerShell,
}

impl ShellType {
    pub fn format_with_sentinel(&self, command: &str) -> String {
        match self {
            // Unix: Use semicolon and $?
            Self::Bash => format!("{}; echo \"IRONGRAPH_CMD_DONE:$?\"\n", command),
            // Windows CMD: Use ampersand and %ERRORLEVEL%
            Self::Cmd => format!("{} & echo IRONGRAPH_CMD_DONE:%ERRORLEVEL%\r\n", command),
            // PowerShell: Use semicolon and $LASTEXITCODE
            Self::PowerShell => format!("{}; Write-Host \"IRONGRAPH_CMD_DONE:$LASTEXITCODE\"\r\n", command),
        }
    }
}
