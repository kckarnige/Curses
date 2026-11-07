import { showContextMenu } from "./contextmenu_helper.js";
const { invoke } = window.__TAURI__.core;
const { openPath } = window.__TAURI__.opener;
const { appDataDir, join } = window.__TAURI__.path;
const { Command } = window.__TAURI__.shell;
const { relaunch } = window.__TAURI__.process;
const { readTextFile } = window.__TAURI__.fs;


// Keep one Blob URL per <img>.
const previewUrls = new WeakMap();


async function getCursorDirectory() {
  return join(await appDataDir(), "cursors");
}

async function getCursorTheme(themeName) {
  const infPath = await join(
    await getCursorDirectory(),
    themeName,
    "install.inf"
  );

  return invoke("read_cursor_inf", {
    path: infPath
  });
}

async function applyCursorTheme(theme) {
  const cursors = await getCursorTheme(theme);

  console.log("Installing:", theme);
  console.log(cursors);

  await invoke("apply_cursor_theme", {
    theme,
    cursors
  });

  console.log("Applied cursor theme:", theme);
}

async function showCursor(
  theme,
  cursorName,
  image
) {
  if (!cursorName || !image) {
    return;
  }

  const cursorFile = await join(
    await getCursorDirectory(),
    theme,
    cursorName
  );

  console.log(
    "Loading cursor:",
    cursorFile
  );

  const bytes = await invoke(
    "cursor_preview",
    {
      path: cursorFile
    }
  );

  const previousUrl =
    previewUrls.get(image);

  if (previousUrl) {
    URL.revokeObjectURL(
      previousUrl
    );
  }

  const blob = new Blob(
    [new Uint8Array(bytes)],
    {
      type: "image/png"
    }
  );

  const url =
    URL.createObjectURL(blob);

  previewUrls.set(
    image,
    url
  );

  image.src = url;
}

async function loadCursorThemes() {
  const themes = await invoke("list_cursor_themes");

  console.log("Themes:", themes);

  // Automatically create a container for all cursor themes.
  let themeList = document.getElementById("cursorThemes");

  if (!themeList) {
    themeList = document.createElement("div");
    themeList.id = "cursorThemes";
    document.body.appendChild(themeList);
  }

  const previewRoles = [
    ["Arrow", "#cursor1"],
    ["Hand", "#cursor2"],
    ["AppStarting", "#cursor3"],
    ["Wait", "#cursor4"],
    ["IBeam", "#cursor5"],
    ["No", "#cursor6"]
  ];

  const themeCount = themes.length;
  document.getElementById("curCount").innerText = themeCount;
  var themeCounter = 0;
  for (const theme of themes) {
    console.log("Creating preview for:", theme);

    // Create the entire preview block from scratch.
    const element = document.createElement("div");

    element.id = "curPreview-container";

    element.innerHTML = `
      <div id="curPreview-content" theme="${theme}">
        <div id="curPreviews">
          <img id="cursor1" class="cursor-preview" />
          <img id="cursor2" class="cursor-preview" />
          <img id="cursor3" class="cursor-preview" />
          <img id="cursor4" class="cursor-preview" />
          <img id="cursor5" class="cursor-preview" />
          <img id="cursor6" class="cursor-preview" />
        </div>
        <p id="curName"></p>
      </div>
    `;

    try {
      const metadataPath = await join(await getCursorDirectory(), theme, "pack-info.json");
      const metadataJSON = JSON.parse(await readTextFile(metadataPath))
      console.log(metadataJSON.name)
      element.querySelector("#curName").innerText = metadataJSON.name;
      if (metadataJSON.author.name != (null || undefined || "")) {
        element.querySelector("#curName").innerText += " by " + metadataJSON.author.name
      }
    } catch (error) {
      console.error("This theme doesn't seem to have metadata. Using folder name instead.\n\nMore Info:" + error)
      element.querySelector("#curName").innerText = theme;
    }

    element.addEventListener("contextmenu", async (event) => {
      try {
        const selected = await showContextMenu(event, [
          { id: "apply", label: "Apply theme" },
          { id: "open", label: "Open folder" },
          { type: "separator" },
          { id: "export", label: "Export Cursepack" },
          { id: "delete", label: "Delete" }
        ]);

        switch (selected) {
          case "apply":
            applyTheme(theme);
            break;

          case "open":
            openThemeFolder(theme);
            break;

          case "export":
            exportTheme(theme);
            break;

          case "delete":
            deleteTheme(theme);
            break;
        }
      } catch (error) {
        console.error("Context menu failed:", error);
      }
    });

    themeList.appendChild(element);

    try {
      const cursors = await getCursorTheme(theme);

      console.log(`Cursors for ${theme}:`, cursors);

      const promises = previewRoles.map(
        async ([role, selector]) => {
          const cursorName = cursors[role];
          const image = element.querySelector(selector);

          console.log(
            `${theme}: ${role} ->`,
            cursorName
          );

          if (!cursorName) {
            console.warn(
              `${theme} has no ${role} cursor`
            );

            return;
          }

          if (!image) {
            console.error(
              `Could not find ${selector} for ${theme}`
            );

            return;
          }

          try {
            await showCursor(
              theme,
              cursorName,
              image
            );
          } catch (error) {
            console.error(
              `Failed to preview ${theme} ${role}:`,
              error
            );
          }
        }
      );

      await Promise.allSettled(promises);
    } catch (error) {
      console.error(
        `Failed to parse theme "${theme}":`,
        error
      );
    }
    themeCounter += 1
    console.log(themeCounter)
    if (themeCounter == themeCount) {
      themeList.classList.add("loaded");
      document.getElementById("loading").remove();
    }
  }
}

window.addEventListener("DOMContentLoaded", async () => {
  document
    .addEventListener("click", async (event) => {
      const button = event.target.closest("#curPreview-content");
      if (!button) return;

      await applyCursorTheme(button.getAttribute("theme"))
    });
  document
    .addEventListener("click", async (event) => {
      const button = event.target.closest("#cursorDir-button");
      if (!button) return;

      const curDir = await getCursorDirectory()
      await openPath(curDir);
    });

  document
    .querySelector(".themes-button")
    .addEventListener("click", async () => {
      document.getElementById("sidebar").setAttribute("openTab", "themes")
      document.getElementById("main").innerHTML = '<p id="loading">Loading...</p><div id="cursorThemes"></div>'
      try {
        await loadCursorThemes();
      } catch (error) {
        console.error("Failed to load cursor themes:", error);
      }
    });

  document
    .querySelector(".packager-button")
    .addEventListener("click", async () => {
      document.getElementById("sidebar").setAttribute("openTab", "packager")
      const html = `
      <div id="cursorPackager">
        <button id="cursorDir-button">
          <svg id="icon"><use href="#folder" /></svg><p>Cursor Folder</p>
        </button>
      </div>
      `
      document.getElementById("main").innerHTML = html
    });

  document
    .querySelector(".info-button")
    .addEventListener("click", async () => {
      document.getElementById("sidebar").setAttribute("openTab", "about")
      const html = `
      <div id="curseInfoStart">
        <div id="curseIconContainer">
          <img id="curseIcon" src="./icon.svg" />
        </div>
        <div id="curseInfoText">
          <p id="appName">Curses Cursor Manager</p>
          <p id="appSubtitle">Made out of necessity, boredom, and "Why in curses doesn't this already exist?".</p>
        </div>
      </div>
      <div id="curseInfoMid">
        <div id="curseInfoText">
          <p>
            <span id="header">Why use this when MouseX exists?</span>
            <br><br>

            &#8226; MouseX uses it's own rendering engine for it's cursor, which is great for the features they provide, but most cursors available for download don't really use them. On top of that, most cursors are just ports of cursors made for native Windows, which sort-of defeats the whole purpose of buying the software to begin with, unless you're <i>IMMEASURABLY</i> lazy and want to just hit "Subscribe" and forget about it, which is understandable.
            <br><br>
            &#8226; Unlike a native Windows cursor, since MouseX isn't loaded on system startup, your set cursor only applies when you sign-in, which isn't really a problem, but it would just make your system feel inconsistent, especially if you go crazy with Wallpaper Engine
            <br><br>
            &#8226; If you have a cursor pack you found online with a \`install.inf\` file, there's a high-likelyhood it will work with Curses.
            <br><br>
            &#8226; In a perfect universe, if your cursor is packed as a \`.cursepack\`, you can be sure it's safe, compared to a random Steam Workshop download, or shady looking \`install.inf\` file.
            <br><br>
            &#8226; It'll save you 3 bucks.
        </div>
      </div>
      <div id="curseInfoMid">
        <div id="curseInfoText">
          <p>
            <span id="header">Why do you hate MouseX so much?</span>
            <br><br>

            <b>I don't!</b> I actually bought it, and it's a great piece of software, I'm just a nut who likes his stuff a certain way, and at least for my personal preferences, MouseX just doesn't make sense; I just like my stuff functioning as close to native as possible, like how Wallpaper Engine just sits behind your desktop icons as if it were always a feature, but can't show wallpapers on the lock screen, but gives you the option to capture a screenshot of the wallpaper as a work-around. That being said, if you haven't checked out MouseX, you can find it on Steam.</p>
        </div>
      </div>
      <div id="curseInfoEnd">
        <div id="curseInfoText">
          <p id="copyright">&#169; KiCKinTheBucket, Robert E. Reyes</p>
        </div>
      </div>
      <br>
      `
      document.getElementById("main").innerHTML = html
    });

  try {
    await loadCursorThemes();
  } catch (error) {
    console.error("Failed to load cursor themes:", error);
  }
});

var palette = await invoke("get_accent_palette");
var accentCSS = `
    :root {
      --accent-darkest: ${palette[0]};
      --accent-darker: ${palette[1]};
      --accent-dark: ${palette[2]};
      --accent: ${palette[3]};
      --accent-bright: ${palette[4]};
      --accent-brighter: ${palette[5]};
      --accent-brightest: ${palette[6]};
    }
    `
document.getElementById("systemAccents").innerHTML = accentCSS;

// Disable reload and context menu --> https://github.com/orgs/tauri-apps/discussions/3844#discussioncomment-9291465
document.addEventListener('keydown', function (event) {
  if (
    event.key === 'F5' ||
    (event.ctrlKey && event.key === 'r')
  ) {
    event.preventDefault();
  }
});

document.addEventListener('contextmenu', function (event) {
  event.preventDefault();
});