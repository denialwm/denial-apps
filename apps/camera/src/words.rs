//! Camera's words in Simplified Chinese, by their English
//! (`denial_look::words`).

pub static CHINESE: &[(&str, &str)] = &[
    // Taking photos.
    ("Timer off", "定时关闭"),
    ("Timer {seconds} s", "定时 {seconds} 秒"),
    ("{seconds} s", "{seconds} 秒"),
    ("Developing", "正在处理"),
    ("Developing {count} photos", "正在处理 {count} 张照片"),
    ("Saved to Pictures", "已保存到图片"),
    // The gallery.
    ("Photos", "照片"),
    ("Photo", "照片"),
    ("1 photo", "1 张照片"),
    ("{count} photos", "{count} 张照片"),
    ("{index} of {count}", "第 {index} 张，共 {count} 张"),
    (
        "No photos yet. The ones you take appear here.",
        "还没有照片。拍摄的照片会显示在这里。",
    ),
    ("Taken", "拍摄时间"),
    ("Size", "尺寸"),
    ("File", "文件"),
    ("Exposure", "曝光"),
    (
        "{sensor} · {speed} · gain {gain}×",
        "{sensor} · {speed} · 增益 {gain}×",
    ),
    (" · digital {digital}×", " · 数字 {digital}×"),
    ("Delete", "删除"),
    ("Cancel", "取消"),
    ("Delete this photo?", "删除这张照片？"),
    (
        "It is removed from Pictures/Camera. This cannot be undone.",
        "它会从 Pictures/Camera 中移除，且无法撤销。",
    ),
    ("Photo deleted", "照片已删除"),
    ("Not deleted: {error}", "未删除：{error}"),
    // Dates.
    ("Today", "今天"),
    ("Yesterday", "昨天"),
    ("Earlier", "更早"),
    ("January", "1月"),
    ("February", "2月"),
    ("March", "3月"),
    ("April", "4月"),
    ("May", "5月"),
    ("June", "6月"),
    ("July", "7月"),
    ("August", "8月"),
    ("September", "9月"),
    ("October", "10月"),
    ("November", "11月"),
    ("December", "12月"),
    ("{day} {month}", "{month}{day}日"),
    ("{day} {month} {year}", "{year}年{month}{day}日"),
    (
        "{day} {month} {year}, {time}",
        "{year}年{month}{day}日 {time}",
    ),
];

#[cfg(test)]
mod tests {
    #[test]
    fn table_is_sound() {
        assert_eq!(
            denial_look::words::faults(super::CHINESE),
            Vec::<String>::new()
        );
    }
}
