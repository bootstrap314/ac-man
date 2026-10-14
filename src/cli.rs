use crate::{
    AcController, AppSnapshot, DeviceSnapshot, MODE_NAMES, Result, SWING_NAMES, SettingsChange,
    default_data_directory, problem,
};
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
};
use zeroize::Zeroizing;

const EXAMPLES: &str = "Examples:\n  ac-man login\n  ac-man list\n  ac-man 1 status\n  ac-man 3 fan 37\n  ac-man \"Bedroom\" boost on\n  ac-man 2 set --temperature 23.5 --mode cool --fan auto\n  ac-man 3 rename \"Office\"\n  ac-man 3 id 7\n\nPasswords are prompted without echo; there is no --password argument.";

#[derive(Parser)]
#[command(name="ac-man",version,about="Control Midea ACs through SmartHome cloud",after_help=EXAMPLES)]
struct Cli {
    #[arg(
        long,
        global = true,
        help = "Directory shared with the menu-bar app (or AC_MAN_DATA_DIR)"
    )]
    data_dir: Option<PathBuf>,
    #[arg(long, global = true, help = "Print machine-readable JSON")]
    json: bool,
    #[command(subcommand)]
    command: Top,
}
#[derive(Subcommand)]
enum Top {
    /// Sign in and securely save the login for later commands.
    Login {
        /// SmartHome account email; prompted if omitted.
        email: Option<String>,
        /// Read one password line from stdin for automation; never a command argument.
        #[arg(long, requires = "email")]
        password_stdin: bool,
    },
    /// Remove the saved local login and session from the OS credential store.
    Logout,
    /// Show assigned integer IDs and room names without needing a network connection.
    #[command(alias = "devices")]
    List {
        #[arg(long, help = "Also fetch live settings from SmartHome")]
        refresh: bool,
    },
    #[command(external_subcommand)]
    Device(Vec<String>),
}
#[derive(Parser)]
#[command(name="ac-man <DEVICE>",about="Use an assigned integer ID or a quoted room name",after_help=EXAMPLES)]
struct DeviceCli {
    #[arg(long, global = true)]
    json: bool,
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: DeviceCommand,
}
#[derive(Subcommand)]
enum DeviceCommand {
    /// Read current settings and supported controls.
    Status,
    /// Turn the unit on.
    On,
    /// Turn the unit off.
    Off,
    /// Set target temperature in Celsius, using half-degree steps.
    #[command(alias = "temp")]
    Temperature { value: f64 },
    /// Set fan speed to 1–100 percent, or auto.
    Fan { value: String },
    /// Select the operating mode.
    Mode {
        #[arg(value_enum)]
        value: Mode,
    },
    /// Select the swing direction.
    Swing {
        #[arg(value_enum)]
        value: Swing,
    },
    /// Enable or disable turbo/Boost.
    Boost {
        #[arg(value_enum)]
        value: Toggle,
    },
    /// Change several settings in one confirmed command.
    Set(SetOptions),
    /// Change the room name on this computer.
    Rename { name: String },
    /// Assign a unique positive integer ID to this unit.
    Id { value: u32 },
}
#[derive(Clone, ValueEnum)]
enum Toggle {
    On,
    Off,
}
#[derive(Clone, ValueEnum)]
enum Mode {
    Auto,
    Cool,
    Dry,
    Heat,
    Fan,
    #[value(name = "smart-dry")]
    SmartDry,
}
#[derive(Clone, ValueEnum)]
enum Swing {
    Off,
    Horizontal,
    Vertical,
    Both,
}
impl Mode {
    fn value(&self) -> i32 {
        match self {
            Self::Auto => 1,
            Self::Cool => 2,
            Self::Dry => 3,
            Self::Heat => 4,
            Self::Fan => 5,
            Self::SmartDry => 6,
        }
    }
}
impl Swing {
    fn value(&self) -> i32 {
        match self {
            Self::Off => 0,
            Self::Horizontal => 3,
            Self::Vertical => 12,
            Self::Both => 15,
        }
    }
}
impl Toggle {
    fn enabled(&self) -> bool {
        matches!(self, Self::On)
    }
}
#[derive(Args)]
struct SetOptions {
    #[arg(long, value_enum)]
    power: Option<Toggle>,
    #[arg(long)]
    temperature: Option<f64>,
    #[arg(long, value_enum)]
    mode: Option<Mode>,
    #[arg(long)]
    fan: Option<String>,
    #[arg(long, value_enum)]
    swing: Option<Swing>,
    #[arg(long, value_enum)]
    boost: Option<Toggle>,
}
fn fan_value(value: &str) -> Result<i32> {
    if value.eq_ignore_ascii_case("auto") {
        return Ok(102);
    }
    value
        .trim_end_matches('%')
        .parse::<i32>()
        .ok()
        .filter(|v| (1..=100).contains(v))
        .ok_or_else(|| problem("Fan speed must be a whole percentage from 1 to 100, or auto."))
}
fn change(command: DeviceCommand) -> Result<SettingsChange> {
    let mut c = SettingsChange::default();
    match command {
        DeviceCommand::On => c.power = Some(true),
        DeviceCommand::Off => c.power = Some(false),
        DeviceCommand::Temperature { value } => c.temperature = Some(value),
        DeviceCommand::Fan { value } => c.fan = Some(fan_value(&value)?),
        DeviceCommand::Mode { value } => c.mode = Some(value.value()),
        DeviceCommand::Swing { value } => c.swing = Some(value.value()),
        DeviceCommand::Boost { value } => c.boost = Some(value.enabled()),
        DeviceCommand::Set(o) => {
            c.power = o.power.map(|v| v.enabled());
            c.temperature = o.temperature;
            c.mode = o.mode.map(|v| v.value());
            c.fan = o.fan.map(|v| fan_value(&v)).transpose()?;
            c.swing = o.swing.map(|v| v.value());
            c.boost = o.boost.map(|v| v.enabled());
        }
        _ => return Err(problem("Choose a device control command.")),
    }
    if c.temperature
        .is_some_and(|v| !v.is_finite() || (v * 2.0).fract() != 0.0)
    {
        return Err(problem(
            "Temperature must use finite half-degree Celsius steps.",
        ));
    }
    if c.power.is_none()
        && c.temperature.is_none()
        && c.mode.is_none()
        && c.fan.is_none()
        && c.swing.is_none()
        && c.boost.is_none()
    {
        return Err(problem(
            "Provide at least one setting, such as --temperature 23.",
        ));
    }
    Ok(c)
}
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}
fn print_devices(snapshot: &AppSnapshot, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(snapshot).map_err(|_| problem("Cannot format status."))?
        );
    } else {
        println!("ID  ROOM                          STATUS");
        for d in &snapshot.devices {
            let status = if d.online {
                format!(
                    "{} · {}°C · Fan {}{}",
                    if d.power { "On" } else { "Off" },
                    d.temperature.map_or("—".into(), |v| v.to_string()),
                    if d.fan == 102 {
                        "Auto".into()
                    } else {
                        format!("{}%", d.fan)
                    },
                    if d.boost { " · Boost" } else { "" }
                )
            } else if !d.error.is_empty() {
                clean(&d.error)
            } else {
                "Not refreshed".into()
            };
            println!("{:<3} {:<29} {}", d.local_id, clean(&d.label), status);
        }
    }
    Ok(())
}
fn print_device(d: &DeviceSnapshot, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(d).map_err(|_| problem("Cannot format status."))?
        );
        return Ok(());
    }
    let name = |values: &[(i32, &str)], v: i32| {
        values
            .iter()
            .find(|(id, _)| *id == v)
            .map(|(_, s)| *s)
            .unwrap_or("Unknown")
            .to_owned()
    };
    println!("{} · ID {}", clean(&d.label), d.local_id);
    println!(
        "Power: {}\nTarget: {}°C\nRoom: {}°C\nMode: {}\nFan: {}\nSwing: {}\nBoost: {}",
        if d.power { "On" } else { "Off" },
        d.temperature.map_or("—".into(), |v| v.to_string()),
        d.indoor.map_or("—".into(), |v| v.to_string()),
        name(MODE_NAMES, d.mode),
        if d.fan == 102 {
            "Auto".into()
        } else {
            format!("{}%", d.fan)
        },
        name(SWING_NAMES, d.swing),
        if d.boost { "On" } else { "Off" }
    );
    Ok(())
}
fn prompt_account() -> Result<String> {
    if !io::stdin().is_terminal() {
        return Err(problem(
            "Supply the account email when stdin is not a terminal.",
        ));
    }
    eprint!("SmartHome email: ");
    io::stderr()
        .flush()
        .map_err(|_| problem("Cannot write the login prompt."))?;
    let mut value = String::new();
    io::stdin()
        .read_line(&mut value)
        .map_err(|_| problem("Cannot read the account email."))?;
    Ok(value.trim().to_owned())
}
async fn run_cli(cli: Cli) -> Result<()> {
    let mut directory = cli.data_dir.unwrap_or_else(default_data_directory);
    let mut json = cli.json;
    if let Top::Device(ref arguments) = cli.command {
        let selector = arguments
            .first()
            .ok_or_else(|| problem("Choose a device."))?
            .clone();
        let d = DeviceCli::try_parse_from(
            std::iter::once("ac-man <DEVICE>".to_owned()).chain(arguments.iter().skip(1).cloned()),
        )
        .unwrap_or_else(|e| e.exit());
        if let Some(path) = d.data_dir {
            directory = path;
        }
        json |= d.json;
        // Parse and validate options before touching Keychain or the network.
        let control = match d.command {
            DeviceCommand::Status | DeviceCommand::Rename { .. } | DeviceCommand::Id { .. } => None,
            command => Some(change(command)?),
        };
        let controller = AcController::new(directory.to_string_lossy().into())?;
        if let Some(change) = control {
            return print_devices(&controller.control(selector, change).await?, json);
        }
        // Reparse only local/status commands, preserving clap's own validation/help.
        let d = DeviceCli::parse_from(
            std::iter::once("ac-man <DEVICE>".to_owned()).chain(arguments.iter().skip(1).cloned()),
        );
        return match d.command {
            DeviceCommand::Status => print_device(&controller.device_status(selector).await?, json),
            DeviceCommand::Rename { name } => {
                print_devices(&controller.rename(selector, name).await?, json)
            }
            DeviceCommand::Id { value } => {
                print_devices(&controller.assign_id(selector, value).await?, json)
            }
            _ => unreachable!(),
        };
    }
    let controller = AcController::new(directory.to_string_lossy().into())?;
    match cli.command {
        Top::Login {
            email,
            password_stdin,
        } => {
            let account = match email {
                Some(v) => v,
                None => prompt_account()?,
            };
            let password = if password_stdin {
                let mut value = Zeroizing::new(String::new());
                io::stdin()
                    .read_line(&mut value)
                    .map_err(|_| problem("Cannot read the password from stdin."))?;
                while value.ends_with(['\r', '\n']) {
                    value.pop();
                }
                value
            } else {
                Zeroizing::new(rpassword::prompt_password("SmartHome password: ").map_err(|_|problem("Cannot open a terminal for hidden password entry. Use --password-stdin with an email for automation."))?)
            };
            let snapshot = controller
                .login(account, password.to_string(), true)
                .await?;
            if !json {
                println!("Signed in. Login and session saved in the OS credential store.");
            }
            print_devices(&snapshot, json)
        }
        Top::Logout => {
            let s = controller.logout().await?;
            if json {
                print_devices(&s, true)
            } else {
                println!("Saved login and session removed.");
                Ok(())
            }
        }
        Top::List { refresh } => {
            let s = if refresh {
                controller.refresh().await?
            } else {
                controller.snapshot().await?
            };
            print_devices(&s, json)?;
            if refresh && s.devices.iter().any(|d| !d.error.is_empty()) {
                return Err(problem("One or more ACs could not be refreshed."));
            }
            Ok(())
        }
        Top::Device(_) => unreachable!(),
    }
}
pub async fn run() -> ExitCode {
    match run_cli(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {}", clean(&e.to_string()));
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_first_syntax_and_named_rooms() {
        for args in [
            ["ac-man", "3", "fan", "37"],
            ["ac-man", "Bedroom", "boost", "on"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            assert!(matches!(cli.command, Top::Device(_)));
        }
        let d = DeviceCli::try_parse_from([
            "ac-man <DEVICE>",
            "set",
            "--fan",
            "auto",
            "--temperature",
            "23.5",
            "--boost",
            "off",
            "--json",
        ])
        .unwrap();
        assert!(d.json);
        let c = change(d.command).unwrap();
        assert_eq!(c.fan, Some(102));
        assert_eq!(c.temperature, Some(23.5));
        assert_eq!(c.boost, Some(false));
    }
    #[test]
    fn login_never_accepts_password_argument() {
        assert!(
            Cli::try_parse_from([
                "ac-man",
                "login",
                "user@example.test",
                "--password",
                "secret"
            ])
            .is_err()
        );
        assert!(Cli::try_parse_from(["ac-man", "login"]).is_ok());
        assert!(
            Cli::try_parse_from(["ac-man", "login", "user@example.test", "--password-stdin"])
                .is_ok()
        );
    }
    #[test]
    fn invalid_percentages_and_empty_set_fail() {
        for v in ["0", "101", "102", "-1", "37.5", "NaN"] {
            assert!(fan_value(v).is_err());
        }
        assert_eq!(fan_value("37%").unwrap(), 37);
        assert_eq!(fan_value("AUTO").unwrap(), 102);
        let d = DeviceCli::try_parse_from(["ac-man <DEVICE>", "set"]).unwrap();
        assert!(change(d.command).is_err());
    }
    #[test]
    fn terminal_labels_are_sanitized() {
        assert_eq!(clean("Room\x1b\n\r"), "Room");
    }
}
