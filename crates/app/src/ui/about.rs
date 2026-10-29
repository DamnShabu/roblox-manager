//! About Roblox Manager.

use adw::prelude::*;

/// The about dialog, over `parent` when there is one.
pub fn show(parent: Option<&gtk::Window>) {
    let about = adw::AboutDialog::builder()
        .application_name("Roblox Manager")
        .application_icon(crate::APP_ID)
        .developer_name("mujō")
        .version(env!("CARGO_PKG_VERSION"))
        .website("https://github.com/DamnShabu/roblox-manager")
        .issue_url("https://github.com/DamnShabu/roblox-manager/issues")
        .license_type(gtk::License::MitX11)
        .comments(
            "Several Roblox accounts, launched into one server.\n\n\
             Roblox's rules treat running several clients at once as a policy violation, and \
             community reports tie it to anti-cheat flags. The second client only ever starts \
             when you ask for it.",
        )
        .build();
    about.add_legal_section(
        "Cordial",
        Some("The runtime each client runs in: Stacked, a fork of Cordial."),
        gtk::License::Gpl30,
        None,
    );
    about.add_legal_section(
        "Material Symbols",
        Some("The app's icons, by Google."),
        gtk::License::Apache20,
        None,
    );
    about.present(parent);
}
