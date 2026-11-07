const { invoke } = window.__TAURI__.core;

export async function showContextMenu(event, items) {
    event.preventDefault();

    const scale = window.devicePixelRatio || 1;

    return invoke("show_context_menu", {
        x: Math.round(event.screenX * scale),
        y: Math.round(event.screenY * scale),
        items: items.map(item =>
            item.type === "separator"
                ? { type: "separator" }
                : {
                    type: "item",
                    id: item.id,
                    label: item.label,
                    enabled: item.enabled ?? true
                }
        )
    });
}
