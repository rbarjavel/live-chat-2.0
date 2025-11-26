use std::path::PathBuf;

/// Get the cookies file path
#[must_use]
pub fn get_cookies_path() -> PathBuf {
    let config_dir = dirs::config_dir().expect("Could not determine config directory");

    let app_config = config_dir.join("image-chat");

    // Create directory if it doesn't exist
    std::fs::create_dir_all(&app_config).expect("Could not create config directory");

    app_config.join("cookies.txt")
}

/// Check if cookies file exists
#[must_use]
pub fn has_cookies() -> bool {
    get_cookies_path().exists()
}

/// Print setup instructions
pub fn print_cookie_setup_instructions() {
    let cookies_path = get_cookies_path();

    println!("\n{}", "=".repeat(60));
    println!("COOKIE AUTHENTICATION SETUP");
    println!("{}", "=".repeat(60));
    println!("\nTo download from sites requiring authentication, you need to");
    println!("export your browser cookies to:");
    println!("\n  {}", cookies_path.display());
    println!("\n{}", "-".repeat(60));
    println!("📌 FIREFOX INSTRUCTIONS:");
    println!("{}", "-".repeat(60));
    println!("1. Install browser extension:");
    println!("   https://addons.mozilla.org/en-US/firefox/addon/cookies-txt/");
    println!("2. Navigate to the site you want to download from (YouTube, Twitter, etc.)");
    println!("3. Make sure you're logged in");
    println!("4. Click the extension icon → 'Export' → 'Current site'");
    println!("5. Save the file as: {}", cookies_path.display());
    println!("\n{}", "-".repeat(60));
    println!("📌 CHROME/CHROMIUM INSTRUCTIONS:");
    println!("{}", "-".repeat(60));
    println!("1. Install extension:");
    println!(
        "   https://chrome.google.com/webstore/detail/get-cookiestxt/bgaddhkoddajcdgocldbbfleckgcbcid"
    );
    println!("2. Navigate to the site you want to download from");
    println!("3. Make sure you're logged in");
    println!("4. Click the extension icon → 'Export'");
    println!("5. Save the file as: {}", cookies_path.display());
    println!("\n{}", "-".repeat(60));
    println!("⚠️  SECURITY WARNING:");
    println!("{}", "-".repeat(60));
    println!("Cookies contain authentication credentials. Keep this file secure!");
    println!("Do NOT share it or commit it to version control.");
    println!("\n{}", "=".repeat(60));
}
