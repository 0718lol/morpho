export type Lang = "zh" | "en";

const dict: Record<Lang, Record<string, string>> = {
  zh: {
    tagline: "万物皆可转",
    engines: "引擎",
    dropTitle: "拖拽文件到这里，或点击选择",
    dropSub: "图片 · 音视频 · 文档 · 表格 · 演示 · PDF — 全程本地处理，隐私不出机",
    convertTo: "转换为",
    quality: "质量",
    presetDefault: "默认",
    presetWechat: "微信 / 小体积",
    presetWeb: "网页 / 均衡",
    presetArchive: "归档 / 高质量",
    output: "输出",
    outputSame: "与源文件相同目录",
    pickDir: "选择目录",
    reset: "重置",
    convert: "开始转换",
    queue: "转换队列",
    history: "最近转换",
    clear: "清空",
    localOnly: "100% 离线 · MIT 开源",
    remove: "移除",
    waiting: "等待中",
    starting: "启动中…",
    done: "完成",
    failed: "失败",
    cancelled: "已取消",
    openDir: "打开位置",
    pickFiles: "选择文件",
    noTargets: "所选文件的可用输出格式没有交集，请分开转换",
    browserHint: "浏览器预览模式：转换功能需在桌面应用中使用",
    engineMissing: "部分引擎缺失",
    engineOk: "全部引擎就绪",
    converting: "转换中",
    catImage: "图片",
    catVideo: "视频",
    catAudio: "音频",
    catText: "文本",
    catDocument: "文档",
    catSheet: "表格",
    catSlide: "演示",
    catPdf: "PDF",
    searchablePdf: "可搜索 PDF",
    sameFormat: "与源格式相同",
  },
  en: {
    tagline: "everything converts",
    engines: "Engines",
    dropTitle: "Drop files here, or click to browse",
    dropSub: "Images · Audio/Video · Docs · Sheets · Slides · PDF — processed locally, fully private",
    convertTo: "Convert to",
    quality: "Quality",
    presetDefault: "Default",
    presetWechat: "WeChat / small",
    presetWeb: "Web / balanced",
    presetArchive: "Archive / high quality",
    output: "Output",
    outputSame: "Same folder as source",
    pickDir: "Choose folder",
    reset: "Reset",
    convert: "Convert",
    queue: "Queue",
    history: "Recent",
    clear: "Clear",
    localOnly: "100% offline · MIT licensed",
    remove: "Remove",
    waiting: "Waiting",
    starting: "Starting…",
    done: "Done",
    failed: "Failed",
    cancelled: "Cancelled",
    openDir: "Reveal",
    pickFiles: "Pick files",
    noTargets: "No common output format for the selected files — convert them separately",
    browserHint: "Browser preview: conversion requires the desktop app",
    engineMissing: "some engines missing",
    engineOk: "all engines ready",
    converting: "Converting",
    catImage: "Image",
    catVideo: "Video",
    catAudio: "Audio",
    catText: "Text",
    catDocument: "Docs",
    catSheet: "Sheets",
    catSlide: "Slides",
    catPdf: "PDF",
    searchablePdf: "Searchable PDF",
    sameFormat: "same as source",
  },
};

export let lang: Lang = (localStorage.getItem("morpho-lang") as Lang) || "zh";

export function setLang(l: Lang) {
  lang = l;
  localStorage.setItem("morpho-lang", l);
}

export function t(key: string): string {
  return dict[lang][key] ?? dict.zh[key] ?? key;
}

export function applyStatic(root: ParentNode = document) {
  root.querySelectorAll<HTMLElement>("[data-i18n]").forEach((el) => {
    el.textContent = t(el.dataset.i18n!);
  });
}
