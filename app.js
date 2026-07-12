// VoxLink 官方网站 - 动态下载链接获取
// 自动从 GitHub Releases API 获取最新版本下载链接
// 支持全平台：macOS / Windows / iOS / Android

(function () {
  "use strict";

  // GitHub 仓库信息（部署时替换为实际仓库）
  const GITHUB_OWNER = "voxlink";
  const GITHUB_REPO = "voxlink";
  const GITHUB_API = `https://api.github.com/repos/${GITHUB_OWNER}/${GITHUB_REPO}/releases/latest`;

  // DOM 元素 - 桌面端
  const downloadMacOS = document.getElementById("download-macos");
  const downloadWindows = document.getElementById("download-windows");
  const macosFileSpan = document.querySelector(".macos-file");
  const windowsFileSpan = document.querySelector(".windows-file");

  // DOM 元素 - 移动端
  const downloadIOS = document.getElementById("download-ios");
  const downloadAndroid = document.getElementById("download-android");
  const iosFileSpan = document.querySelector(".ios-file");
  const androidFileSpan = document.querySelector(".android-file");

  /**
   * 从 GitHub Releases API 获取最新版本信息
   */
  async function fetchLatestRelease() {
    try {
      console.log("[VoxLink] 正在获取最新版本...");

      const response = await fetch(GITHUB_API, {
        headers: {
          Accept: "application/vnd.github.v3+json",
        },
      });

      if (!response.ok) {
        throw new Error(`GitHub API 返回 ${response.status}: ${response.statusText}`);
      }

      const release = await response.json();
      console.log(`[VoxLink] 最新版本: ${release.tag_name}`);
      console.log(`[VoxLink] 资产数量: ${release.assets ? release.assets.length : 0}`);

      return release;
    } catch (error) {
      console.error("[VoxLink] 获取版本信息失败:", error);
      return null;
    }
  }

  /**
   * 从资产列表中匹配下载链接（全平台）
   */
  function matchAssets(assets) {
    if (!assets || assets.length === 0) {
      return {
        macosIntel: null,
        macosSilicon: null,
        windows: null,
        ios: null,
        androidApk: null,
        androidAab: null,
      };
    }

    const result = {
      macosIntel: null,
      macosSilicon: null,
      windows: null,
      ios: null,
      androidApk: null,
      androidAab: null,
    };

    for (const asset of assets) {
      const name = asset.name.toLowerCase();
      const url = asset.browser_download_url;

      if (name.endsWith(".dmg")) {
        if (name.includes("aarch64") || name.includes("arm64") || name.includes("apple-silicon")) {
          result.macosSilicon = { name: asset.name, url, size: asset.size };
        } else if (name.includes("x64") || name.includes("x86_64") || name.includes("intel")) {
          result.macosIntel = { name: asset.name, url, size: asset.size };
        } else {
          if (!result.macosIntel) {
            result.macosIntel = { name: asset.name, url, size: asset.size };
          }
        }
      } else if (name.endsWith(".msi")) {
        result.windows = { name: asset.name, url, size: asset.size };
      } else if (name.endsWith(".exe") && (name.includes("setup") || name.includes("install"))) {
        if (!result.windows) {
          result.windows = { name: asset.name, url, size: asset.size };
        }
      } else if (name.endsWith(".ipa")) {
        result.ios = { name: asset.name, url, size: asset.size };
      } else if (name.endsWith(".apk")) {
        result.androidApk = { name: asset.name, url, size: asset.size };
      } else if (name.endsWith(".aab")) {
        result.androidAab = { name: asset.name, url, size: asset.size };
      }
    }

    return result;
  }

  /**
   * 格式化文件大小
   */
  function formatSize(bytes) {
    if (!bytes) return "";
    const mb = bytes / (1024 * 1024);
    return mb >= 1 ? `${mb.toFixed(1)} MB` : `${(bytes / 1024).toFixed(0)} KB`;
  }

  /**
   * 更新下载按钮
   */
  function updateDownloadButtons(assets, release) {
    const version = release ? release.tag_name : "";

    // --- macOS ---
    if (assets.macosIntel || assets.macosSilicon) {
      const macosAsset = assets.macosSilicon || assets.macosIntel;
      downloadMacOS.href = macosAsset.url;
      downloadMacOS.classList.remove("opacity-50", "cursor-not-allowed");
      downloadMacOS.classList.add("cursor-pointer");

      if (macosFileSpan) {
        const sizeStr = formatSize(macosAsset.size);
        macosFileSpan.textContent = `.dmg ${version ? `(${version})` : ""} ${sizeStr}`;
      }

      if (assets.macosSilicon && assets.macosIntel) {
        addSubLinks(downloadMacOS, [
          { label: "Apple Silicon", url: assets.macosSilicon.url, size: formatSize(assets.macosSilicon.size) },
          { label: "Intel", url: assets.macosIntel.url, size: formatSize(assets.macosIntel.size) },
        ]);
      }
    } else {
      downloadMacOS.href = "#";
      downloadMacOS.classList.add("opacity-50", "cursor-not-allowed");
      if (macosFileSpan) macosFileSpan.textContent = "即将推出";
    }

    // --- Windows ---
    if (assets.windows) {
      downloadWindows.href = assets.windows.url;
      downloadWindows.classList.remove("opacity-50", "cursor-not-allowed");
      downloadWindows.classList.add("cursor-pointer");

      if (windowsFileSpan) {
        const sizeStr = formatSize(assets.windows.size);
        windowsFileSpan.textContent = `.msi ${version ? `(${version})` : ""} ${sizeStr}`;
      }
    } else {
      downloadWindows.href = "#";
      downloadWindows.classList.add("opacity-50", "cursor-not-allowed");
      if (windowsFileSpan) windowsFileSpan.textContent = "即将推出";
    }

    // --- iOS ---
    if (downloadIOS && assets.ios) {
      downloadIOS.href = assets.ios.url;
      downloadIOS.classList.remove("opacity-50", "cursor-not-allowed");
      downloadIOS.classList.add("cursor-pointer");

      if (iosFileSpan) {
        const sizeStr = formatSize(assets.ios.size);
        iosFileSpan.textContent = `.ipa ${version ? `(${version})` : ""} ${sizeStr}`;
      }
    } else if (downloadIOS) {
      downloadIOS.href = "#";
      downloadIOS.classList.add("opacity-50", "cursor-not-allowed");
      if (iosFileSpan) iosFileSpan.textContent = "即将推出";
    }

    // --- Android ---
    if (downloadAndroid && assets.androidApk) {
      downloadAndroid.href = assets.androidApk.url;
      downloadAndroid.classList.remove("opacity-50", "cursor-not-allowed");
      downloadAndroid.classList.add("cursor-pointer");

      if (androidFileSpan) {
        const sizeStr = formatSize(assets.androidApk.size);
        androidFileSpan.textContent = `.apk ${version ? `(${version})` : ""} ${sizeStr}`;
      }
    } else if (downloadAndroid) {
      downloadAndroid.href = "#";
      downloadAndroid.classList.add("opacity-50", "cursor-not-allowed");
      if (androidFileSpan) androidFileSpan.textContent = "即将推出";
    }
  }

  /**
   * 添加子链接（用于多架构选择）
   */
  function addSubLinks(parentElement, links) {
    const existing = parentElement.querySelector(".sub-links");
    if (existing) existing.remove();

    const container = document.createElement("div");
    container.className = "sub-links absolute left-0 right-0 top-full mt-2 glass rounded-xl p-2 opacity-0 group-hover:opacity-100 transition-opacity";

    links.forEach((link) => {
      const a = document.createElement("a");
      a.href = link.url;
      a.className = "block px-4 py-2 rounded-lg text-sm text-white/70 hover:text-white hover:bg-white/10 transition-colors";
      a.textContent = `${link.label} - ${link.size}`;
      container.appendChild(a);
    });

    const wrapper = document.createElement("div");
    wrapper.className = "relative group inline-block";
    parentElement.parentNode.insertBefore(wrapper, parentElement);
    wrapper.appendChild(parentElement);
    wrapper.appendChild(container);
  }

  /**
   * 下载追踪
   */
  function trackDownload(platform, version) {
    console.log(`[VoxLink] 下载 - 平台: ${platform}, 版本: ${version}`);
  }

  // 绑定下载事件 - 桌面端
  if (downloadMacOS) {
    downloadMacOS.addEventListener("click", function () {
      trackDownload("macOS", downloadMacOS.href.includes("releases") ? "latest" : "unavailable");
    });
  }
  if (downloadWindows) {
    downloadWindows.addEventListener("click", function () {
      trackDownload("Windows", downloadWindows.href.includes("releases") ? "latest" : "unavailable");
    });
  }

  // 绑定下载事件 - 移动端
  if (downloadIOS) {
    downloadIOS.addEventListener("click", function () {
      trackDownload("iOS", downloadIOS.href.includes("releases") ? "latest" : "unavailable");
    });
  }
  if (downloadAndroid) {
    downloadAndroid.addEventListener("click", function () {
      trackDownload("Android", downloadAndroid.href.includes("releases") ? "latest" : "unavailable");
    });
  }

  // 页面加载时初始化
  async function init() {
    console.log("[VoxLink] 网站初始化...");

    const release = await fetchLatestRelease();

    if (release && release.assets) {
      const assets = matchAssets(release.assets);
      updateDownloadButtons(assets, release);
      console.log("[VoxLink] 下载链接已更新（全平台）");
    } else {
      console.warn("[VoxLink] 无法获取最新版本，显示占位符");
      updateDownloadButtons(
        { macosIntel: null, macosSilicon: null, windows: null, ios: null, androidApk: null, androidAab: null },
        null
      );
    }
  }

  // 启动
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();