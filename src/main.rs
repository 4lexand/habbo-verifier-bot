use poise::serenity_prelude as serenity;
use rand::{distributions::Alphanumeric, Rng};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::Mutex;

#[derive(Deserialize, Debug)]
struct HabboProfile {
    motto: Option<String>,
    #[serde(rename = "profileVisible")]
    profile_visible: bool,
}

pub struct Data {
    pending_verifications: Arc<Mutex<HashMap<u64, (String, String)>>>,
}

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

// 1. COMANDO DE SETUP (Solo para Administradores)
#[poise::command(slash_command, required_permissions = "ADMINISTRATOR")]
async fn setup_verificacion(ctx: Context<'_>) -> Result<(), Error> {
    let components = vec![serenity::CreateActionRow::Buttons(vec![
        serenity::CreateButton::new("btn_start_verify")
            .label("Vincular cuenta de Habbo")
            .style(serenity::ButtonStyle::Success),
    ])];

    let msg = "¡Bienvenido! Para tener acceso completo al servidor, haz clic en el botón de abajo y escribe tu nombre de usuario de Habbo.";

    let reply = poise::CreateReply::default()
        .content(msg)
        .components(components);

    ctx.send(reply).await?;
    Ok(())
}

// 2. ESCUCHADOR GLOBAL DE EVENTOS (Vigila los botones y formularios)
async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    _data: &Data,
) -> Result<(), Error> {
    if let serenity::FullEvent::InteractionCreate { interaction } = event {
        
        // A. Si alguien hace clic en el botón estático inicial...
        if let Some(mci) = interaction.as_message_component() {
            if mci.data.custom_id == "btn_start_verify" {
                // Creamos la ventana emergente (Modal)
                let components = vec![serenity::CreateActionRow::InputText(
                    serenity::CreateInputText::new(
                        serenity::InputTextStyle::Short,
                        "Tu nombre en Habbo",
                        "habbo_username"
                    )
                    .placeholder("Ejemplo: Alexx_17")
                    .required(true)
                    .min_length(1)
                    .max_length(50)
                )];

                let modal = serenity::CreateModal::new("modal_verify", "Verificación de Habbo")
                    .components(components);

                // Mostramos la ventana al usuario
                let _ = mci.create_response(ctx, serenity::CreateInteractionResponse::Modal(modal)).await;
            }
        }

        // B. Si alguien termina de llenar la ventana emergente y le da a Enviar...
        if let Some(modal) = interaction.as_modal_submit() {
            if modal.data.custom_id == "modal_verify" {
                // Sacamos el nombre que escribieron en la cajita
                let mut habbo_name = String::new();
                if let Some(row) = modal.data.components.first() {
                    if let Some(serenity::ActionRowComponent::InputText(input)) = row.components.first() {
                        habbo_name = input.value.clone().unwrap_or_default();
                    }
                }

                let clean_habbo_name = habbo_name.trim().trim_start_matches('@').to_string();

                let random_string: String = rand::thread_rng()
                    .sample_iter(&Alphanumeric)
                    .take(6)
                    .map(char::from)
                    .collect();
                let code = format!("HBB-{}", random_string.to_uppercase());

                // Usamos el ID del usuario para que nadie más pueda usar su botón de confirmar
                let user_id = modal.user.id;
                let button_id = format!("btn_confirm_{}", user_id);

                let msg = format!(
                    "¡Hola! Para vincular tu cuenta de Habbo (**{}**), copia y pega este código en tu Misión (Motto) dentro del juego:\n\n`{}`\n\nUna vez lo hayas guardado, haz clic en el botón de abajo para verificar.",
                    clean_habbo_name, code
                );

                let components = vec![serenity::CreateActionRow::Buttons(vec![
                    serenity::CreateButton::new(&button_id)
                        .label("✅ Ya puse el código, Verificar")
                        .style(serenity::ButtonStyle::Success),
                ])];

                // Respondemos de forma EFÍMERA (oculta, solo para ese usuario)
                let _ = modal.create_response(ctx, serenity::CreateInteractionResponse::Message(
                    serenity::CreateInteractionResponseMessage::new()
                        .ephemeral(true)
                        .content(msg)
                        .components(components)
                )).await;

                // C. Nos quedamos esperando a que presione el botón de "Ya puse el código"
                while let Some(mci_confirm) = serenity::ComponentInteractionCollector::new(ctx)
                    .author_id(user_id)
                    .timeout(Duration::from_secs(600))
                    .filter({
                        let button_id = button_id.clone();
                        move |mci| mci.data.custom_id == button_id
                    })
                    .await
                {
                    let _ = mci_confirm.create_response(ctx, serenity::CreateInteractionResponse::Defer(
                        serenity::CreateInteractionResponseMessage::new().ephemeral(true)
                    )).await;

                    let url = format!("https://www.habbo.es/api/public/users?name={}", clean_habbo_name);
                    
                    let client = reqwest::Client::new();
                    match client.get(&url)
                        .header(reqwest::header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
                        .send()
                        .await 
                    {
                        Ok(resp) => {
                            if resp.status().is_success() {
                                let profile: HabboProfile = resp.json().await.unwrap_or(HabboProfile { motto: None, profile_visible: false });
                                
                                if !profile.profile_visible {
                                    let _ = mci_confirm.create_followup(ctx, serenity::CreateInteractionResponseFollowup::new()
                                        .content("❌ **Error:** Tu perfil de Habbo está en **privado**. Hazlo público en tus ajustes del juego para poder leer tu misión.")).await;
                                    continue;
                                }

                                if let Some(motto) = profile.motto {
                                    if motto.contains(&code) {
                                        // Éxito: Verificación lograda
                                        let _ = mci_confirm.create_followup(ctx, serenity::CreateInteractionResponseFollowup::new()
                                            .content(format!("🎉 **¡Verificación exitosa!** Tu cuenta de Discord ahora está vinculada a **{}**.", clean_habbo_name))).await;
                                        
                                        if let Some(guild_id) = mci_confirm.guild_id {
                                            let edit_member = serenity::EditMember::new().nickname(clean_habbo_name.clone());
                                            let _ = guild_id.edit_member(ctx, user_id, edit_member).await;
                                            
                                            // Leemos el rol desde el .env
                                            let role_id_str = std::env::var("VERIFIED_ROLE_ID").unwrap_or_else(|_| "0".to_string());
                                            let role_id_num = role_id_str.parse::<u64>().unwrap_or(0);
                                            
                                            if role_id_num != 0 {
                                                let role_id = serenity::RoleId::new(role_id_num);
                                                let _ = ctx.http.add_member_role(guild_id, user_id, role_id, Some("Verificación Habbo exitosa")).await;
                                            }
                                        }
                                        break; 
                                    } else {
                                        let _ = mci_confirm.create_followup(ctx, serenity::CreateInteractionResponseFollowup::new()
                                            .content(format!("⚠️ La misión actual de tu perfil es `{}`. Asegúrate de poner exactamente `{}` y guarda los cambios en el juego.", motto, code))).await;
                                    }
                                } else {
                                    let _ = mci_confirm.create_followup(ctx, serenity::CreateInteractionResponseFollowup::new()
                                        .content("⚠️ Tu misión de Habbo está vacía. Pon el código y vuelve a intentarlo.")).await;
                                }

                            } else if resp.status().as_u16() == 404 {
                                let _ = mci_confirm.create_followup(ctx, serenity::CreateInteractionResponseFollowup::new()
                                    .content("❌ No se encontró el usuario en Habbo. Revisa que el nombre esté bien escrito.")).await;
                                break;
                            } else {
                                let _ = mci_confirm.create_followup(ctx, serenity::CreateInteractionResponseFollowup::new()
                                    .content(format!("⚠️ Error del servidor de Habbo (Código: {}). Intenta en unos minutos.", resp.status().as_u16()))).await;
                            }
                        },
                        Err(e) => {
                            let _ = mci_confirm.create_followup(ctx, serenity::CreateInteractionResponseFollowup::new()
                                .content(format!("⚠️ Error de conexión interno del bot: {}", e))).await;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let token = std::env::var("DISCORD_TOKEN")
        .expect("Falta la variable de entorno DISCORD_TOKEN en el archivo .env");

    let intents = serenity::GatewayIntents::non_privileged() 
        | serenity::GatewayIntents::MESSAGE_CONTENT;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            // Actualizamos los comandos para usar el de setup
            commands: vec![setup_verificacion()],
            // Inyectamos nuestro escuchador de eventos
            event_handler: |ctx, event, framework, data| {
                Box::pin(event_handler(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                println!("¡Bot de Habbo conectado, comandos y eventos registrados!");
                Ok(Data {
                    pending_verifications: Arc::new(Mutex::new(HashMap::new())),
                })
            })
        })
        .build();

    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await
        .unwrap();

    if let Err(why) = client.start().await {
        println!("Error crítico del cliente: {:?}", why);
    }
}