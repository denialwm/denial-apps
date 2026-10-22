//! Messages' words in Simplified Chinese, by their English
//! (`denial_look::words`); its notifier's too.

pub static CHINESE: &[(&str, &str)] = &[
    // The list.
    ("Messages", "信息"),
    ("Conversations", "对话"),
    ("Search", "搜索"),
    ("Nothing found", "未找到"),
    ("Messages · {count}", "信息 · {count}"),
    ("Conversations · {count}", "对话 · {count}"),
    ("Unread · {count}", "未读 · {count}"),
    ("Pinned", "已置顶"),
    ("Earlier", "更早"),
    ("New message", "新信息"),
    ("New conversation", "新对话"),
    ("No messages yet", "还没有信息"),
    (
        "Texts you send and receive with this SIM appear here.",
        "用这张 SIM 卡收发的短信会显示在这里。",
    ),
    ("Draft: {draft}", "草稿：{draft}"),
    ("You: {text}", "我：{text}"),
    ("You", "我"),
    ("Pin", "置顶"),
    ("Unpin", "取消置顶"),
    ("Archive", "归档"),
    ("Unarchive", "取消归档"),
    ("Archived", "已归档"),
    ("No archived conversations", "没有已归档的对话"),
    (
        "An archived conversation comes back here when a new message arrives in it.",
        "已归档的对话收到新信息时会回到这里。",
    ),
    ("Mark read", "标为已读"),
    ("Mark unread", "标为未读"),
    ("Not marked read", "未能标为已读"),
    ("Delete", "删除"),
    ("Cancel", "取消"),
    ("Not deleted", "未删除"),
    ("Not changed", "未更改"),
    ("Delete this conversation?", "删除这个对话？"),
    (
        "Its messages are removed from this phone.",
        "它的信息会从这部手机中移除。",
    ),
    ("Delete this message?", "删除这条信息？"),
    (
        "It is removed from this phone. {name} keeps their copy.",
        "它会从这部手机中移除，{name} 仍保留其副本。",
    ),
    // Writing.
    ("To", "收件人"),
    ("Name or phone number", "姓名或电话号码"),
    ("Contacts and recent", "联系人和最近"),
    ("No contact matches", "没有匹配的联系人"),
    ("Not a phone number", "不是电话号码"),
    ("Send to {number}", "发送至 {number}"),
    ("Message", "信息"),
    ("This sender takes no replies", "此发件人不接受回复"),
    // A message's state.
    ("Sending", "正在发送"),
    ("Sending…", "正在发送…"),
    ("Sent", "已发送"),
    ("Delivered", "已送达"),
    ("{time} · Delivered", "{time} · 已送达"),
    ("Received", "已接收"),
    ("Not sent", "未发送"),
    ("Not sent. Tap to try again", "未发送。轻点重试"),
    ("Not delivered", "未送达"),
    ("Expired", "已过期"),
    ("Retry", "重试"),
    ("Try again", "重试"),
    ("Copy", "复制"),
    ("Copied", "已复制"),
    ("Code copied", "已复制验证码"),
    ("Number copied", "已复制号码"),
    // The modem.
    ("Connecting to the modem service", "正在连接调制解调器服务"),
    (
        "Messages may not use the modem service",
        "信息无权使用调制解调器服务",
    ),
    ("The modem is not working", "调制解调器无法工作"),
    ("No SIM card", "没有 SIM 卡"),
    ("The SIM is locked", "SIM 卡已锁定"),
    ("No mobile network", "没有移动网络"),
    // Blocking.
    ("Block", "屏蔽"),
    ("Unblock", "取消屏蔽"),
    ("Blocked", "已屏蔽"),
    ("Unblocked", "已取消屏蔽"),
    ("Block {name}?", "屏蔽 {name}？"),
    (
        "Their messages go to Blocked messages in settings, without a notification.",
        "对方的信息会进入设置中的“已屏蔽信息”，不发通知。",
    ),
    ("Blocked messages", "已屏蔽信息"),
    ("Blocked numbers", "已屏蔽号码"),
    ("No blocked messages", "没有已屏蔽信息"),
    ("No blocked numbers", "没有已屏蔽号码"),
    (
        "Messages from blocked numbers are kept here, without a notification.",
        "来自已屏蔽号码的信息保存在这里，不发通知。",
    ),
    // Settings.
    ("Settings", "设置"),
    ("SIM for messages", "用于信息的 SIM 卡"),
    ("Delivery reports", "送达报告"),
    (
        "Ask the network to confirm each message arrived",
        "请网络确认每条信息已送达",
    ),
    ("Sample conversations", "示例对话"),
    (
        "Made-up chats to try the app. Nothing in them is sent",
        "用于试用的虚构对话，不会发送任何内容",
    ),
    ("Try it", "试试"),
    ("Back in Messages", "已回到信息"),
    // The notifier.
    ("Open", "打开"),
    ("Mark as read", "标为已读"),
    // Numbers' kinds.
    ("Mobile", "手机"),
    ("Home", "住宅"),
    ("Work", "工作"),
    ("Main", "总机"),
    ("Fax", "传真"),
    ("Pager", "寻呼机"),
    ("Other", "其他"),
    // Dates.
    ("Today", "今天"),
    ("Yesterday", "昨天"),
    ("Sun", "周日"),
    ("Mon", "周一"),
    ("Tue", "周二"),
    ("Wed", "周三"),
    ("Thu", "周四"),
    ("Fri", "周五"),
    ("Sat", "周六"),
    ("Jan", "1月"),
    ("Feb", "2月"),
    ("Mar", "3月"),
    ("Apr", "4月"),
    ("May", "5月"),
    ("Jun", "6月"),
    ("Jul", "7月"),
    ("Aug", "8月"),
    ("Sep", "9月"),
    ("Oct", "10月"),
    ("Nov", "11月"),
    ("Dec", "12月"),
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
