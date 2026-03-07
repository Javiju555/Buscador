use ashpd::desktop::global_shortcuts::{
    Activated, BindShortcutsOptions, GlobalShortcuts, NewShortcut,
};
use ashpd::desktop::{CreateSessionOptions, Session};
use futures_lite::StreamExt;

const SHORTCUT_ID: &str = "toggle-launcher";

/// Arranca un hilo dedicado con un runtime tokio que:
/// 1. Crea una sesión de global shortcuts vía xdg-desktop-portal
/// 2. Registra Ctrl+Space como trigger preferido
/// 3. Escucha el signal `Activated` y llama a toggle_main_window
pub fn spawn_portal_shortcut_listener(app: tauri::AppHandle) {
    std::thread::Builder::new()
        .name("portal-shortcut".into())
        .spawn(move || {
            let rt: tokio::runtime::Runtime =
                match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(error) => {
                        log::error!(
                            "No se pudo crear runtime tokio para portal shortcuts: {error}"
                        );
                        return;
                    }
                };

            rt.block_on(async move {
                if let Err(error) = run_portal_listener(&app).await {
                    log::error!("Error en portal shortcut listener: {error}");
                }
            });
        })
        .ok();
}

async fn run_portal_listener(app: &tauri::AppHandle) -> ashpd::Result<()> {
    let portal: GlobalShortcuts = GlobalShortcuts::new().await?;
    log::info!(
        "Portal GlobalShortcuts conectado (version {})",
        portal.version()
    );

    // Suscribirse a Activated ANTES de bind para no perder ningun signal
    let mut stream = std::pin::pin!(portal.receive_activated().await?);
    log::info!("Suscripcion a señal Activated lista");

    let session: Session<GlobalShortcuts> = portal
        .create_session(CreateSessionOptions::default())
        .await?;
    log::info!("Sesion de global shortcuts creada");

    let shortcut = NewShortcut::new(SHORTCUT_ID, "Toggle Buscador launcher")
        .preferred_trigger("CTRL+SHIFT+space");

    let request = portal
        .bind_shortcuts(&session, &[shortcut], None, BindShortcutsOptions::default())
        .await?;

    match request.response() {
        Ok(response) => {
            log::info!("Shortcuts registrados via portal: {response:?}");
        }
        Err(error) => {
            log::error!("Error en la respuesta de bind_shortcuts: {error}");
            return Err(error);
        }
    }

    log::info!("Escuchando activaciones de shortcut via portal...");

    loop {
        match StreamExt::next(&mut stream).await {
            Some(activated) => {
                let activated: Activated = activated;
                let id: &str = activated.shortcut_id();
                log::info!("Portal shortcut activado: id={id}");
                if id == SHORTCUT_ID {
                    if let Err(error) = super::toggle_main_window(app) {
                        log::error!(
                            "Error toggling launcher desde portal shortcut: {error}"
                        );
                    }
                }
            }
            None => {
                log::warn!("Stream de activacion de portal terminó inesperadamente");
                break;
            }
        }
    }

    Ok(())
}
