// The desktop shell's own words: its dialogs, notices, menu-bar item and approval window. The page inside the window has its
// own translations (apps/web/src/locales); the four notification texts here are the same words as the web app's `notify.*`,
// so the owner reads one sentence whichever of the two shows it.

export const LANGUAGES = ["en", "hi"];

export const STRINGS = {
  en: {
    "approve.title": "Approve release",
    "approve.message": "Approve this release?",
    "approve.refuse": "Refuse",
    "approve.yes": "Approve with Touch ID",
    "approve.reason": "Approve release {digest} to {recipient}",
    "desc.recipient": "Recipient: {recipient}",
    "desc.digest": "Digest: {digest}",
    "desc.files": "These exact files will be signed and handed over, nothing else:",
    "desc.file": "  {name}  {bytes} bytes  sha256 {sha}",
    "desc.disclosure": "Cumulative disclosure: {after} of {total} project files would have been shown to this recipient ({before} earlier release(s)).",
    "desc.notChecked": "  Not checked: {list}",
    "desc.withheld": "Withheld and never released:",
    "desc.none": "  (none)",
    "desc.notEstablished": "This release does not establish:",
    "desc.nothingListed": "  (nothing listed)",
    "deadline.expired": "This release has expired and can no longer be approved.",
    "deadline.validFor": "This release can be approved for another {time}.",
    "deadline.soon": "! Only a few minutes are left.",
    "deadline.unknown": "The deadline could not be read here; the service refuses an approval that is too late.",
    "notify.approval_waiting": "Something is waiting for your approval.",
    "notify.ready_to_review": "A review is ready.",
    "notify.team_question": "An expert on your team has a question for you.",
    "notify.failed": "A review stopped before it finished.",
    "notice.restarted.title": "Kakurega restarted",
    "notice.restarted.body": "The local companion stopped and was started again. Anything that was running at that moment was stopped; nothing was sent anywhere.",
    "notice.reloaded.title": "Kakurega reloaded the window",
    "notice.reloaded.body": "The window crashed and was reloaded. Your data and any running reviews were not affected.",
    "notice.needsKey.title": "A release is waiting, but approving needs a signing key",
    "notice.needsKey.body": "Run `kaku approver enroll` once in a terminal, then approve the release there with `kaku approve`.",
    "error.stopped.title": "Kakurega stopped",
    "error.stopped.loop": "The local companion keeps stopping, so it was not started again. Your data is safe. Quit and open Kakurega again; if it keeps happening, check the activity log.",
    "error.stopped.restart": "The local companion could not be restarted: {why}",
    "error.window.title": "Kakurega window stopped",
    "error.window.body": "The window keeps crashing, so it was not reloaded again. Quit and open Kakurega again.",
    "error.start.title": "Kakurega could not start",
    "error.changed.title": "This copy of Kakurega has been changed",
    "error.changed.body": "A part of the app is not the one it was built with ({what}), so nothing was started. Your data was not touched. Install Kakurega again from the published download and check its SHA-256 value.",
    "unresponsive.title": "Kakurega is not responding",
    "unresponsive.message": "The window is not responding.",
    "unresponsive.wait": "Wait",
    "unresponsive.reload": "Reload the window",
    "tray.running": "Kakurega is running",
    "tray.stopped": "Kakurega has stopped. Quit and open it again.",
    "tray.nothing": "Nothing is waiting for you",
    "tray.waiting.one": "1 thing is waiting for you",
    "tray.waiting.many": "{n} things are waiting for you",
    "tray.open": "Open Kakurega",
    "tray.inbox": "Waiting for you",
    "tray.inboxCount": "Waiting for you ({n})",
    "tray.notify": "Tell me when something needs me",
    "tray.quit": "Quit Kakurega",
    "tray.capture": "Take a picture of the screen for the chat…",
    "capture.confirm.message": "Attach this picture of your screen to the chat?",
    "capture.confirm.detail": "It stays on this computer: Kakurega reads the text in it here. That text is sent to the model only when you send a message with it attached. Lines that hold a key or a password are withheld.",
    "capture.confirm.yes": "Attach it",
    "capture.confirm.no": "Discard",
    "capture.denied.message": "Kakurega is not allowed to see the screen",
    "capture.denied.detail": "Allow it in System Settings, Privacy & Security, Screen & System Audio Recording, then choose the menu item again. Nothing was captured.",
    "capture.failed.message": "The picture was not attached",
    "capture.failed.none": "No picture of the screen could be taken.",
    "capture.failed.large": "The picture is larger than a file the chat takes (8 MB).",
    "capture.failed.refused": "The service on this computer did not take the picture.",
    "capture.preview.title": "Picture of your screen",
  },
  hi: {
    "approve.title": "रिलीज़ को मंज़ूरी दें",
    "approve.message": "क्या यह रिलीज़ स्वीकृत करें?",
    "approve.refuse": "अस्वीकार करें",
    "approve.yes": "Touch ID से स्वीकृत करें",
    "approve.reason": "रिलीज़ {digest} को {recipient} के लिए स्वीकृत करें",
    "desc.recipient": "प्राप्तकर्ता: {recipient}",
    "desc.digest": "डाइजेस्ट: {digest}",
    "desc.files": "केवल ये फ़ाइलें, ठीक इसी रूप में, हस्ताक्षरित करके सौंपी जाएँगी, और कुछ नहीं:",
    "desc.file": "  {name}  {bytes} बाइट  sha256 {sha}",
    "desc.disclosure": "कुल प्रकटीकरण: प्रोजेक्ट की {total} फ़ाइलों में से {after} इस प्राप्तकर्ता को दिखाई जा चुकी होंगी ({before} पिछली रिलीज़)।",
    "desc.notChecked": "  जाँचा नहीं गया: {list}",
    "desc.withheld": "रोकी गई, कभी जारी नहीं होंगी:",
    "desc.none": "  (कुछ नहीं)",
    "desc.notEstablished": "यह रिलीज़ यह सिद्ध नहीं करती:",
    "desc.nothingListed": "  (कुछ सूचीबद्ध नहीं)",
    "deadline.expired": "यह रिलीज़ समाप्त हो गई है और अब स्वीकृत नहीं की जा सकती।",
    "deadline.validFor": "यह रिलीज़ अभी {time} और स्वीकृत की जा सकती है।",
    "deadline.soon": "! केवल कुछ मिनट बचे हैं।",
    "deadline.unknown": "समय-सीमा यहाँ पढ़ी नहीं जा सकी; बहुत देर से दी गई मंज़ूरी को सेवा अस्वीकार कर देती है।",
    "notify.approval_waiting": "कोई चीज़ आपकी मंज़ूरी का इंतज़ार कर रही है।",
    "notify.ready_to_review": "एक समीक्षा तैयार है।",
    "notify.team_question": "आपकी टीम के एक विशेषज्ञ का आपसे सवाल है।",
    "notify.failed": "एक समीक्षा पूरी होने से पहले रुक गई।",
    "notice.restarted.title": "Kakurega फिर से शुरू हुआ",
    "notice.restarted.body": "स्थानीय सहायक सेवा रुक गई थी और फिर से शुरू की गई। उस समय जो चल रहा था वह रोक दिया गया; कुछ भी कहीं नहीं भेजा गया।",
    "notice.reloaded.title": "Kakurega ने विंडो फिर से लोड की",
    "notice.reloaded.body": "विंडो बंद हो गई थी और फिर से लोड की गई। आपका डेटा और चल रही समीक्षाएँ प्रभावित नहीं हुईं।",
    "notice.needsKey.title": "एक रिलीज़ प्रतीक्षा में है, पर मंज़ूरी के लिए हस्ताक्षर कुंजी चाहिए",
    "notice.needsKey.body": "टर्मिनल में एक बार `kaku approver enroll` चलाएँ, फिर वहीं `kaku approve` से रिलीज़ स्वीकृत करें।",
    "error.stopped.title": "Kakurega रुक गया",
    "error.stopped.loop": "स्थानीय सहायक सेवा बार-बार रुक रही है, इसलिए उसे फिर से शुरू नहीं किया गया। आपका डेटा सुरक्षित है। Kakurega बंद करके फिर खोलें; अगर ऐसा होता रहे तो गतिविधि लॉग देखें।",
    "error.stopped.restart": "स्थानीय सहायक सेवा फिर से शुरू नहीं हो सकी: {why}",
    "error.window.title": "Kakurega की विंडो रुक गई",
    "error.window.body": "विंडो बार-बार बंद हो रही है, इसलिए उसे फिर से लोड नहीं किया गया। Kakurega बंद करके फिर खोलें।",
    "error.start.title": "Kakurega शुरू नहीं हो सका",
    "error.changed.title": "Kakurega की इस प्रति में बदलाव किया गया है",
    "error.changed.body": "ऐप का एक हिस्सा ({what}) वह नहीं है जिसके साथ यह बनाया गया था, इसलिए कुछ भी शुरू नहीं किया गया। आपके डेटा को छुआ नहीं गया। प्रकाशित डाउनलोड से Kakurega फिर से इंस्टॉल करें और उसका SHA-256 मान जाँचें।",
    "unresponsive.title": "Kakurega जवाब नहीं दे रहा",
    "unresponsive.message": "विंडो जवाब नहीं दे रही है।",
    "unresponsive.wait": "प्रतीक्षा करें",
    "unresponsive.reload": "विंडो फिर से लोड करें",
    "tray.running": "Kakurega चल रहा है",
    "tray.stopped": "Kakurega रुक गया है। इसे बंद करके फिर खोलें।",
    "tray.nothing": "कुछ भी आपकी प्रतीक्षा में नहीं है",
    "tray.waiting.one": "1 चीज़ आपकी प्रतीक्षा में है",
    "tray.waiting.many": "{n} चीज़ें आपकी प्रतीक्षा में हैं",
    "tray.open": "Kakurega खोलें",
    "tray.inbox": "आपकी प्रतीक्षा में",
    "tray.inboxCount": "आपकी प्रतीक्षा में ({n})",
    "tray.notify": "जब किसी चीज़ को मेरी ज़रूरत हो तो बताएँ",
    "tray.quit": "Kakurega बंद करें",
    "tray.capture": "चैट के लिए स्क्रीन की तस्वीर लें…",
    "capture.confirm.message": "अपनी स्क्रीन की यह तस्वीर चैट में जोड़ें?",
    "capture.confirm.detail": "यह इसी कंप्यूटर पर रहती है: Kakurega इसका पाठ यहीं पढ़ता है। वह पाठ मॉडल को तभी भेजा जाता है जब आप इसे जोड़कर संदेश भेजते हैं। जिन पंक्तियों में कोई कुंजी या पासवर्ड है, वे रोक ली जाती हैं।",
    "capture.confirm.yes": "जोड़ें",
    "capture.confirm.no": "हटा दें",
    "capture.denied.message": "Kakurega को स्क्रीन देखने की अनुमति नहीं है",
    "capture.denied.detail": "System Settings, Privacy & Security, Screen & System Audio Recording में अनुमति दें, फिर मेनू से दोबारा चुनें। कुछ भी कैप्चर नहीं हुआ।",
    "capture.failed.message": "तस्वीर नहीं जोड़ी गई",
    "capture.failed.none": "स्क्रीन की तस्वीर नहीं ली जा सकी।",
    "capture.failed.large": "तस्वीर चैट में ली जाने वाली फ़ाइल (8 MB) से बड़ी है।",
    "capture.failed.refused": "इस कंप्यूटर की सेवा ने तस्वीर नहीं ली।",
    "capture.preview.title": "आपकी स्क्रीन की तस्वीर",
  },
};

/**
 * The language to show, from the most preferred first: the first one whose language part is one of ours. "hi-IN" and "hi"
 * are Hindi, "en-GB" is English, anything else is passed over, and English is used when nothing matches.
 */
export function pickLanguage(preferred) {
  for (const tag of Array.isArray(preferred) ? preferred : []) {
    if (typeof tag !== "string") continue;
    const base = tag.trim().toLowerCase().split(/[-_]/)[0];
    if (LANGUAGES.includes(base)) return base;
  }
  return "en";
}

/** `t(key, vars)` for one language. A key this language lacks falls back to English; a key nobody has is shown as itself. */
export function translator(lang) {
  const table = STRINGS[lang] ?? STRINGS.en;
  return (key, vars = {}) => {
    const text = table[key] ?? STRINGS.en[key] ?? key;
    return text.replace(/\{(\w+)\}/g, (whole, name) => (Object.hasOwn(vars, name) ? String(vars[name]) : whole));
  };
}

export const english = translator("en");
