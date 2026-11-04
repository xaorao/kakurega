import { captureSelection } from "./capture.js";

const MENU_ID = "kakurega-send-selection";

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.removeAll(() => {
    chrome.contextMenus.create({ id: MENU_ID, title: "Send selection to Kakurega", contexts: ["selection"] });
  });
});

chrome.contextMenus.onClicked.addListener(async (info) => {
  if (info.menuItemId !== MENU_ID) return;
  const result = await captureSelection(info);
  await chrome.action.setBadgeText({ text: result.ok ? "OK" : "!" });
  await chrome.action.setTitle({ title: result.message });
  setTimeout(() => chrome.action.setBadgeText({ text: "" }), 4000);
});
