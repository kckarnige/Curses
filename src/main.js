const { invoke } = window.__TAURI__.core;
const { openPath } = window.__TAURI__.opener;
const { appDataDir, join } = window.__TAURI__.path;
const { Command } = window.__TAURI__.shell;

// Keep one Blob URL per <img>.
const previewUrls = new WeakMap();

async function runPowerShell(script) {
  const { Command } = window.__TAURI__.shell;

  const command = Command.create("powershell", [
    "-NoProfile",
    "-ExecutionPolicy",
    "Bypass",
    "-Command",
    script
  ]);

  const output = await command.execute();

  if (output.code !== 0) {
    console.error(output.stderr);
    throw new Error(output.stderr);
  }

  return output.stdout;
}

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

  for (const theme of themes) {
    console.log("Creating preview for:", theme);

    // Create the entire preview block from scratch.
    const element = document.createElement("div");

    element.id = "curPreview-container";

    element.innerHTML = `
      <div id="curPreview-content" theme="${theme}">
        <p id="curName"></p>

        <div id="curPreviews">
          <img id="cursor1" class="cursor-preview" />
          <img id="cursor2" class="cursor-preview" />
          <img id="cursor3" class="cursor-preview" />
          <img id="cursor4" class="cursor-preview" />
          <img id="cursor5" class="cursor-preview" />
          <img id="cursor6" class="cursor-preview" />
        </div>
      </div>
    `;

    element.querySelector("#curName").innerText = theme;

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
      document.getElementById("main").innerHTML = '<div id="cursorThemes"></div>'
      try {
        await loadCursorThemes();
      } catch (error) {
        console.error("Failed to load cursor themes:", error);
      }
    });

  document
    .querySelector(".settings-button")
    .addEventListener("click", async () => {
      document.getElementById("main").innerHTML = '<p>Settings Test</p><br><button id="cursorDir-button">Cursor Folder</button>'
    });

  document
    .querySelector(".info-button")
    .addEventListener("click", async () => {
      document.getElementById("main").innerHTML = '<p>Info Test</p>'
    });

  try {
    await loadCursorThemes();
  } catch (error) {
    console.error("Failed to load cursor themes:", error);
  }
});
