// Verhindert unter Windows ein zusätzliches Konsolenfenster im Release-Build. NICHT ENTFERNEN!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    framenab_lib::run();
}
