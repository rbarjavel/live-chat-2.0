# Cookie Export Guide for Image Chat

This guide shows how to export browser cookies for authenticating with various sites.

## Why Cookies?

Many platforms (YouTube, Twitter, TikTok) restrict content to logged-in users:
- Private videos
- Age-restricted content
- User-uploaded content
- Region-locked media

Cookies allow yt-dlp to authenticate as you without storing passwords.

## Firefox Setup (Recommended)

### Step 1: Install Extension

Install "cookies.txt" extension:
https://addons.mozilla.org/en-US/firefox/addon/cookies-txt/

### Step 2: Export Cookies

1. Navigate to the site (e.g., youtube.com)
2. **Log in** to your account
3. Click the extension icon in the toolbar
4. Click "Export" → "Current Site" (or "All Sites" for multiple platforms)
5. Save as: `~/.config/image-chat/cookies.txt`

### Step 3: Verify

In Image Chat, run:
```
/check-cookies
```

You should see: ✅ Cookies file found

## Chrome/Chromium Setup

### Step 1: Install Extension

Install "Get cookies.txt" extension:
https://chrome.google.com/webstore/detail/get-cookiestxt/bgaddhkoddajcdgocldbbfleckgcbcid

### Step 2: Export Cookies

1. Navigate to the site (e.g., twitter.com)
2. **Log in** to your account
3. Click the extension icon
4. Click "Export"
5. Save as: `~/.config/image-chat/cookies.txt`

### Step 3: Verify

Run `/check-cookies` in Image Chat.

## Alternative: Manual Cookie Export

If you can't use extensions, you can use browser developer tools:

### Firefox Manual Export

1. Press F12 to open Developer Tools
2. Go to "Storage" tab
3. Expand "Cookies" → Select your site
4. Copy cookies in Netscape format to cookies.txt

### Chrome Manual Export

1. Press F12 to open Developer Tools
2. Go to "Application" tab
3. Expand "Cookies" → Select your site
4. Copy cookies in Netscape format to cookies.txt

## Security Notes

⚠️ **Cookie files contain your authentication credentials!**

- Never share your cookies.txt file
- Don't commit it to git (add to .gitignore)
- Treat it like a password
- Regenerate by re-exporting if compromised
- Cookies expire after some time (weeks/months)

## Platform-Specific Notes

### YouTube
- Supports age-restricted and private videos
- Members-only content requires membership cookies
- Playlists work if they're accessible to your account

### Twitter/X
- Required for most video downloads (even public ones lately)
- Enables downloading videos from protected accounts you follow

### TikTok
- May be required for some regions
- Helps bypass rate limiting

### Instagram
- Required for private accounts and stories
- Must be logged in and following the account

## Troubleshooting

**Problem**: "Sign in to confirm your age" error
- Export cookies after signing in to YouTube

**Problem**: "This video is unavailable" error
- Check if you can access the video in your browser while logged in
- Re-export cookies (they may have expired)

**Problem**: Cookie file not found
- Check file path: Run `/check-cookies`
- Ensure directory exists: `~/.config/image-chat/`
- Check file permissions (must be readable)

**Problem**: Downloads still fail with cookies
- Try exporting "All Sites" instead of "Current Site"
- Check cookie file format (should be Netscape format)
- Verify yt-dlp version is up to date: `yt-dlp --version`

## Updating Cookies

Cookies expire periodically. If downloads start failing:

1. Log out and back in to the site
2. Re-export cookies with the extension
3. Overwrite the old cookies.txt file
4. Run `/check-cookies` to verify

## Advanced: Multiple Platforms

To support multiple platforms simultaneously:

1. Open each site in separate tabs (YouTube, Twitter, TikTok)
2. Log in to each
3. Use "Export All Sites" option
4. All cookies merged into one file

This lets you download from any supported platform without re-exporting.
