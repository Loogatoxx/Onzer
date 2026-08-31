// Empêche l'ouverture d'une console au lancement d'une version compilée.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    onzer_lib::run()
}
