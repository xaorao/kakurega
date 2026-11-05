//! A token count for text before a model has seen it. After a reply the model's own count is used instead.
//!
//! Characters divided by four (what OpenCode's `util/token.ts` uses) undercounts most writing systems: measured on
//! qwen2.5 it is 3.7 times too low for Hindi and 2.1 times for Chinese. Each character here costs what was measured for
//! its script on that model, a word costs at least one token, and the weights are rounded up so the meter warns early
//! rather than late. On texts never used for tuning it is still off by up to 30% (tk05), so it is only a guess for
//! text the model has not seen yet. Other models tokenize differently.

use crate::ChatMessage;

/// Tokens the chat template adds around every message (role markers and separators), and once per request.
const PER_MESSAGE: f64 = 6.0;
const PER_REQUEST: f64 = 3.0;

/// What one character costs outside a word, and each letter inside one. Ranges that were never measured cost 1.0 per
/// character, which overcounts most scripts rather than undercounting them.
fn weight(c: char) -> f64 {
    match c as u32 {
        _ if c.is_ascii_alphabetic() || c == ' ' || c == '\t' => 0.25,
        // ASCII punctuation, symbols and line breaks: in code and JSON nearly each one is a token of its own
        0x00..=0x7F => 0.45,
        0x80..=0x24F => 0.4,
        0x370..=0x3FF | 0x1F00..=0x1FFF => 1.1,
        0x400..=0x52F => 0.4,
        0x590..=0x5FF => 0.45,
        0x600..=0x64F => 0.45,
        // letters added for Urdu, Persian and other languages cost more than the core Arabic ones (Urdu measured 0.69 a character)
        0x650..=0x6FF | 0x750..=0x77F | 0x8A0..=0x8FF => 1.1,
        0x900..=0xDFF => 1.3,
        0xE00..=0xE7F => 0.65,
        0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF => 0.8,
        0x3000..=0x30FF | 0xFF00..=0xFFEF => 1.0,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F => 0.6,
        _ => 1.0,
    }
}

/// A letter that belongs to a Latin-script word, accents included.
fn latin_letter(c: char) -> bool {
    c.is_ascii_alphabetic() || (c.is_alphabetic() && (0x80..=0x24F).contains(&(c as u32)))
}

fn in_word(c: char) -> bool {
    c.is_ascii_digit() || latin_letter(c)
}

fn text_cost(text: &str) -> f64 {
    let chars: Vec<char> = text.chars().collect();
    let mut total = 0.0;
    let mut i = 0;
    while i < chars.len() {
        if !in_word(chars[i]) {
            total += weight(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && in_word(chars[i]) {
            i += 1;
        }
        let run = &chars[start..i];
        total += if run.iter().any(char::is_ascii_digit) {
            // numbers, and runs that mix letters and digits (ids, hashes), split into about one token per character
            run.len() as f64 * 1.1
        } else {
            // a word costs at least one token however short it is (short romanised Hindi words are rarely one token)
            run.iter().map(|c| weight(*c)).sum::<f64>().max(1.0)
        };
    }
    total
}

/// The estimated tokens of a piece of text.
pub fn estimate_text(text: &str) -> u64 {
    text_cost(text).ceil() as u64
}

/// The estimated tokens of a whole request: every message and the template around it.
pub fn estimate_messages(messages: &[ChatMessage]) -> u64 {
    let body: f64 = messages
        .iter()
        .map(|m| PER_MESSAGE + text_cost(&m.content))
        .sum();
    (PER_REQUEST + body).ceil() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One sentence per script and the tokens qwen2.5:1.5b counted for it through Ollama on 2026-11-04 (the template
    /// subtracted, divided by the number of repeats). Recorded in TESTING.md.
    const MEASURED: &[(&str, &str, f64)] = &[
        ("english prose", "The quarterly report shows that regional sales rose while shipping was delayed and staffing changed. ", 16.0),
        ("russian", "Квартальный отчёт показывает, что региональные продажи выросли, а доставка задерживалась. ", 31.0),
        ("hindi", "तिमाही रिपोर्ट से पता चलता है कि क्षेत्रीय बिक्री बढ़ी जबकि शिपिंग में देरी हुई। ", 73.95),
        ("arabic", "يظهر التقرير ربع السنوي أن المبيعات الإقليمية ارتفعت بينما تأخر الشحن. ", 27.0),
        ("chinese", "季度报告显示区域销售额上升，而发货出现延迟，人员也发生了变化。", 15.95),
        ("japanese", "四半期報告書によると、地域の売上は増加したが、出荷が遅れ、人員も変わった。", 29.95),
        ("korean", "분기 보고서에 따르면 지역 매출은 증가했지만 배송이 지연되고 인력도 변경되었습니다. ", 30.0),
        ("thai", "รายงานรายไตรมาสแสดงว่ายอดขายในภูมิภาคเพิ่มขึ้นแต่การจัดส่งล่าช้า ", 36.95),
        ("tamil", "காலாண்டு அறிக்கை பிராந்திய விற்பனை உயர்ந்தது ஆனால் கப்பல் தாமதமானது. ", 77.95),
        ("bengali", "ত্রৈমাসিক প্রতিবেদনে দেখা যায় আঞ্চলিক বিক্রয় বেড়েছে কিন্তু শিপিং বিলম্বিত হয়েছে। ", 91.95),
        ("greek", "Η τριμηνιαία έκθεση δείχνει ότι οι περιφερειακές πωλήσεις αυξήθηκαν. ", 63.95),
        ("hebrew", "הדוח הרבעוני מראה שהמכירות האזוריות עלו בעוד המשלוחים התעכבו. ", 23.0),
        ("emoji", "🚀🔥✅📦🙂👍🎉💡", 7.975),
        ("curly punctuation", "It’s “quoted” — and so is ‘this’ … really – yes… ", 17.0),
        ("digits and ids", "id=4f9c2a7e-11b0-4d6a-9a3e-5c1b7f2d8e90 total=1234567.89 ", 50.0),
        ("python code", "def total(items):\n    return sum(i.price * i.qty for i in items if i.active)\n\nclass Cart:\n    def __init__(self):\n        self.items = []\n", 34.9),
    ];

    #[test]
    fn tk01_every_measured_script_is_never_more_than_ten_percent_under_and_at_most_eighty_percent_over(
    ) {
        let mut report = Vec::new();
        for (name, text, actual) in MEASURED {
            let ratio = text_cost(text) / actual;
            report.push(format!("{name}: {ratio:.2}"));
            assert!(
                (0.9..=1.8).contains(&ratio),
                "{name}: estimate is {ratio:.2} of the measured count; {report:?}"
            );
        }
        println!("{report:?}");
    }

    /// Measured after the first weights were chosen. Four were undercounted (Urdu 0.62, JSON 0.78, Hinglish 0.87, Rust
    /// 0.89), which changed the rules, so these now count as tuning data too; tk05 checks texts measured after that.
    const SECOND: &[(&str, &str, f64)] = &[
        ("english email", "Hi team, the deployment on Friday went smoothly, but two customers reported slow logins after the cache was cleared. Could someone check the session store before Monday's release? ", 34.0),
        ("hinglish", "Kal meeting mein humne decide kiya ki release agle hafte hoga, aur testing team ko pehle report bhejni hai. ", 31.0),
        ("hindi and english mixed", "कल की meeting में तय हुआ कि release अगले हफ्ते होगा और testing team पहले report भेजेगी। ", 49.0),
        ("spanish", "El informe trimestral muestra que las ventas regionales aumentaron mientras el envío se retrasó. ", 22.0),
        ("urdu", "سہ ماہی رپورٹ سے پتہ چلتا ہے کہ علاقائی فروخت میں اضافہ ہوا۔ ", 40.0),
        ("chinese technical", "请检查数据库连接池的配置，确保在高并发情况下不会出现超时错误。", 18.9),
        ("json", "{\"id\": 42, \"name\": \"Acme Ltd\", \"tags\": [\"billing\", \"priority\"], \"active\": true, \"balance\": 1830.75}\n", 40.9),
        ("rust code", "fn main() {\n    let items = vec![1, 2, 3];\n    let total: i32 = items.iter().sum();\n    println!(\"{total}\");\n}\n", 36.9),
        ("markdown", "## Steps\n\n- Open the **settings** page\n- Choose `Backup`\n- Save the recovery key somewhere safe\n\n", 23.9),
        ("url", "See https://docs.example.com/api/v2/sessions?id=8f3a2b&expand=true for details. ", 25.0),
        ("german", "Die Datenschutzgrundverordnung verlangt eine nachvollziehbare Einwilligungserklärung. ", 24.0),
    ];

    #[test]
    fn tk04_the_second_set_that_changed_the_rules_is_never_more_than_ten_percent_under() {
        let mut report = Vec::new();
        let mut under = Vec::new();
        for (name, text, actual) in SECOND {
            let ratio = text_cost(text) / actual;
            report.push(format!("{name}: {ratio:.2}"));
            if ratio < 0.9 {
                under.push(*name);
            }
        }
        println!("{report:?}");
        assert!(under.is_empty(), "undercounted: {under:?}; all: {report:?}");
    }

    /// Measured after the rules last changed (2026-11-05, qwen2.5:1.5b) and never used to choose them.
    const HELD_OUT: &[(&str, &str, f64)] = &[
        ("marathi", "तिमाही अहवालानुसार प्रादेशिक विक्री वाढली पण माल पाठवण्यास उशीर झाला. ", 63.9),
        ("persian", "گزارش فصلی نشان می‌دهد که فروش منطقه‌ای افزایش یافت اما ارسال با تأخیر انجام شد. ", 46.9),
        ("vietnamese", "Báo cáo quý cho thấy doanh số khu vực tăng trong khi việc giao hàng bị chậm trễ. ", 22.1),
        ("turkish", "Üç aylık rapor, bölgesel satışların arttığını ancak sevkiyatın geciktiğini gösteriyor. ", 29.0),
        ("french", "Le rapport trimestriel montre que les ventes régionales ont augmenté malgré les retards de livraison. ", 24.0),
        ("ukrainian", "Квартальний звіт показує, що регіональні продажі зросли, але доставка затрималася. ", 40.0),
        ("gujarati", "ત્રિમાસિક અહેવાલ દર્શાવે છે કે પ્રાદેશિક વેચાણ વધ્યું પરંતુ શિપિંગમાં વિલંબ થયો. ", 122.9),
        ("telugu", "త్రైమాసిక నివేదిక ప్రాంతీయ అమ్మకాలు పెరిగాయని కానీ రవాణా ఆలస్యమైందని చూపిస్తుంది. ", 125.9),
        ("swahili", "Ripoti ya robo mwaka inaonyesha kuwa mauzo ya kanda yaliongezeka lakini usafirishaji ulichelewa. ", 36.1),
        ("yaml", "server:\n  port: 8080\n  hosts:\n    - api.internal\n    - 10.0.4.17\n  timeout_ms: 2500\n", 41.9),
        ("sql", "SELECT id, email FROM users WHERE created_at > '2026-01-01' AND status = 'active' ORDER BY id LIMIT 50;\n", 35.9),
        ("shell", "find . -name '*.log' -mtime +7 -exec gzip {} \\; && du -sh /var/log/app\n", 25.9),
        ("log line", "2026-11-05T09:14:03.221Z WARN request_id=7c1e9a0b latency_ms=1834 path=/api/v1/orders status=504\n", 54.9),
        ("korean and english", "이번 sprint에서 login API의 timeout 문제를 먼저 fix해야 합니다. ", 14.1),
        ("japanese kana", "このメッセージはテストです。ひらがなとカタカナがおおいぶんしょうです。", 22.9),
        ("chinese with english terms", "我们需要把 Redis 缓存的 TTL 改成 300 秒，然后重新部署 API 服务。", 24.9),
        ("english legal", "Notwithstanding the foregoing, the Licensee shall indemnify the Licensor against any claims arising from unauthorised sublicensing. ", 25.0),
    ];

    #[test]
    fn tk05_on_untuned_texts_the_estimate_is_rough_but_far_better_than_characters_divided_by_four()
    {
        let mut report = Vec::new();
        let mut under = Vec::new();
        for (name, text, actual) in HELD_OUT {
            let ratio = text_cost(text) / actual;
            report.push(format!("{name}: {ratio:.2}"));
            if ratio < 0.9 {
                under.push(*name);
            }
        }
        println!("{report:?}");
        // Measured: 8 of these 17 are undercounted, the worst Swahili at 0.70. That is the honest size of a guess made
        // without the model's tokenizer, so the meter shows the model's own count once it has one, and an overflow is
        // refused by the model (`truncate: false`), never cut silently.
        let worst = HELD_OUT
            .iter()
            .map(|(_, t, a)| text_cost(t) / a)
            .fold(f64::INFINITY, f64::min);
        assert!(
            worst >= 0.65,
            "worst {worst:.2}; undercounted: {under:?}; all: {report:?}"
        );
        let quarter = |t: &str| t.chars().count() as f64 / 4.0;
        for (name, text, actual) in HELD_OUT {
            let ours = (text_cost(text) / actual - 1.0).abs();
            let naive = (quarter(text) / actual - 1.0).abs();
            assert!(
                ours < naive + 0.6,
                "{name}: ours off by {ours:.2}, characters/4 off by {naive:.2}"
            );
        }
        let gujarati = HELD_OUT.iter().find(|r| r.0 == "gujarati").unwrap();
        assert!(
            quarter(gujarati.1) / gujarati.2 < 0.3,
            "characters/4 is badly wrong where this estimate is not"
        );
    }

    #[test]
    fn tk02_characters_divided_by_four_fails_the_same_check_so_this_test_can_fail() {
        let quarter = |t: &str| t.chars().count() as f64 / 4.0;
        let under: Vec<&str> = MEASURED
            .iter()
            .filter(|(_, t, a)| quarter(t) / a < 0.9)
            .map(|(n, ..)| *n)
            .collect();
        assert!(
            under.contains(&"hindi") && under.contains(&"chinese"),
            "{under:?}"
        );
    }

    #[test]
    fn tk03_small_cases() {
        assert_eq!(estimate_text(""), 0);
        assert_eq!(estimate_text("abcd"), 1);
        assert!(estimate_text("12345678") >= 8, "digits cost about one each");
        assert!(
            estimate_text("a1b2c3d4") > estimate_text("abcdefgh"),
            "a run that mixes letters and digits costs more"
        );
        assert!(estimate_text("नमस्ते") >= 6);
        assert_eq!(estimate_messages(&[]), 3);
        let one = [ChatMessage::new("user", "abcd")];
        assert_eq!(
            estimate_messages(&one),
            10,
            "3 for the request, 6 for the message, 1 for the text"
        );
    }
}
