//! Sensors' words in Simplified Chinese, by their English
//! (`denial_look::words`).

pub static CHINESE: &[(&str, &str)] = &[
    ("Sensors", "传感器"),
    // Orientation.
    ("Orientation", "方向"),
    ("No reading yet", "尚无读数"),
    ("Tilt the phone to take one", "倾斜手机以获取读数"),
    ("Upright", "竖直向上"),
    ("Upside down", "上下颠倒"),
    ("Left edge up", "左侧朝上"),
    ("Right edge up", "右侧朝上"),
    ("Portrait", "竖屏"),
    ("Landscape", "横屏"),
    // Compass.
    ("Compass", "指南针"),
    ("Turn the phone to take one", "转动手机以获取读数"),
    ("N", "北"),
    ("NE", "东北"),
    ("E", "东"),
    ("SE", "东南"),
    ("S", "南"),
    ("SW", "西南"),
    ("W", "西"),
    ("NW", "西北"),
    (
        "Not calibrated: move the phone in a figure 8",
        "未校准：请将手机按 8 字形移动",
    ),
    (
        "Low accuracy: move the phone in a figure 8",
        "精度低：请将手机按 8 字形移动",
    ),
    ("Medium accuracy", "精度中等"),
    ("High accuracy", "精度高"),
    // Accelerometer and gyroscope.
    ("Accelerometer", "加速度计"),
    ("Level", "水平"),
    ("Tilted {angle}°", "倾斜 {angle}°"),
    ("Gyroscope", "陀螺仪"),
    ("Still", "静止"),
    ("Turning {speed}°/s", "正在转动 {speed}°/秒"),
    ("Pitch", "俯仰"),
    ("Roll", "横滚"),
    ("Yaw", "偏航"),
    // Ambient light.
    ("Ambient light", "环境光"),
    ("Dark", "黑暗"),
    ("Dim", "昏暗"),
    ("Indoors", "室内"),
    ("Bright indoors", "明亮室内"),
    ("Daylight", "日光"),
    ("Direct sun", "阳光直射"),
    ("1 min ago", "1 分钟前"),
    ("Now", "现在"),
    // Proximity.
    ("Proximity", "接近"),
    ("Near", "有物体靠近"),
    ("Clear", "无遮挡"),
    ("Something covers the sensor", "有物体遮住了传感器"),
    ("Nothing in front of the sensor", "传感器前方没有物体"),
    // When there is nothing to show.
    ("The sensor service is not running", "传感器服务未运行"),
    (
        "Readings show here once steinerd starts.",
        "steinerd 启动后，读数会显示在这里。",
    ),
    ("No sensors found", "未找到传感器"),
    (
        "steinerd reaches no sensor on the sensor core.",
        "steinerd 在传感器核心上未找到任何传感器。",
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
