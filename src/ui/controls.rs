//! Controls use vertical labels and bounded widgets so long translations can wrap.
use super::plots::MUTED;
use eframe::egui;
use glenn_extended::model::{Engine, Flap, Planet, Profile, Simulation};
fn slider(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
) {
    ui.label(label);
    let response = ui.add(
        egui::Slider::new(value, range)
            .suffix(suffix)
            .max_decimals(3),
    );
    response.on_hover_text("Значение можно ввести с клавиатуры двойным щелчком по числу");
    ui.add_space(5.0);
}
macro_rules! select {
    ($ui:expr,$label:literal,$value:expr,$kind:ty) => {
        $ui.label($label);
        egui::ComboBox::from_id_salt($label)
            .width($ui.available_width().min(260.0))
            .selected_text($value.label())
            .show_ui($ui, |ui| {
                for option in <$kind>::ALL {
                    ui.selectable_value(&mut $value, *option, option.label());
                }
            });
        $ui.add_space(8.0);
    };
}
pub fn show(ui: &mut egui::Ui, state: &mut Simulation, tab: &mut usize) {
    ui.label(
        egui::RichText::new("ПАРАМЕТРЫ ЭКСПЕРИМЕНТА")
            .size(11.0)
            .color(MUTED),
    );
    ui.horizontal_wrapped(|ui| {
        for (i, label) in ["Поток", "Профиль", "Крыло", "Двигатель"]
            .iter()
            .enumerate()
        {
            ui.selectable_value(tab, i, *label);
        }
    });
    ui.separator();
    match *tab {
        0 => {
            select!(ui, "Среда", state.planet, Planet);
            slider(ui, "Угол атаки", &mut state.alpha, -30.0..=45.0, "°");
            slider(ui, "Скорость", &mut state.velocity, 0.0..=1500.0, " м/с");
            slider(
                ui,
                if state.planet == Planet::Water {
                    "Глубина"
                } else {
                    "Высота"
                },
                &mut state.altitude,
                0.0..=80000.0,
                " м",
            );
            if state.planet == Planet::Custom {
                slider(
                    ui,
                    "Плотность",
                    &mut state.custom_density,
                    0.00001..=2000.0,
                    " кг/м³",
                );
                slider(
                    ui,
                    "Скорость звука",
                    &mut state.custom_speed_of_sound,
                    1.0..=3000.0,
                    " м/с",
                );
            }
            ui.label(egui::RichText::new("SI: скорость в м/с, высота в метрах. На Земле используется послойная модель ISA до 80 км.").small().color(MUTED));
        }
        1 => {
            // Custom is available only after a successful file import.
            ui.label("Семейство профиля");
            egui::ComboBox::from_id_salt("profile")
                .width(ui.available_width().min(260.0))
                .selected_text(state.profile_type.label())
                .show_ui(ui, |ui| {
                    for profile in Profile::ALL {
                        if *profile != Profile::Custom || state.custom_upper.is_some() {
                            ui.selectable_value(&mut state.profile_type, *profile, profile.label());
                        }
                    }
                });
            ui.add_space(10.0);
            if state.profile_type == Profile::Custom {
                ui.label(&state.profile_name);
                ui.small("Координаты загружены из файла. Закрылок и предкрылок остаются доступны.");
            } else {
                slider(ui, "Кривизна", &mut state.camber, 0.0..=10.0, " %");
                slider(
                    ui,
                    "Положение кривизны",
                    &mut state.camber_pos,
                    10.0..=90.0,
                    " %",
                );
                slider(ui, "Толщина", &mut state.thickness, 1.0..=35.0, " %");
                state.profile_name = if state.profile_type == Profile::Naca4 {
                    format!(
                        "NACA {:.0}{:.0}{:02.0}",
                        state.camber,
                        state.camber_pos / 10.0,
                        state.thickness
                    )
                } else {
                    state.profile_type.label().into()
                };
            }
            if state.profile_type == Profile::Cylinder {
                slider(
                    ui,
                    "Вращение цилиндра",
                    &mut state.cylinder_rpm,
                    -10000.0..=10000.0,
                    " об/мин",
                );
            }
            select!(ui, "Закрылок", state.flap_type, Flap);
            slider(
                ui,
                "Отклонение закрылка",
                &mut state.flap_angle,
                -20.0..=45.0,
                "°",
            );
            slider(
                ui,
                "Отклонение предкрылка",
                &mut state.slat_angle,
                0.0..=25.0,
                "°",
            );
        }
        2 => {
            slider(ui, "Корневая хорда", &mut state.chord, 0.1..=15.0, " м");
            slider(ui, "Размах", &mut state.span, 0.2..=80.0, " м");
            slider(ui, "Стреловидность", &mut state.sweep, -20.0..=70.0, "°");
            slider(
                ui,
                "Сужение: концевая / корневая хорда",
                &mut state.taper,
                0.1..=1.0,
                "",
            );
            slider(
                ui,
                "Крутка законцовки (− = washout)",
                &mut state.twist,
                -15.0..=15.0,
                "°",
            );
            slider(ui, "Поперечное V", &mut state.dihedral, -15.0..=20.0, "°");
            ui.checkbox(&mut state.has_winglets, "Законцовки крыла");
        }
        _ => {
            select!(ui, "Тип двигателя", state.engine_type, Engine);
            slider(
                ui,
                "Номинальная тяга",
                &mut state.max_thrust,
                0.0..=2000000.0,
                " Н",
            );
            slider(
                ui,
                "Полная масса (включая топливо)",
                &mut state.aircraft_mass,
                1.0..=1000000.0,
                " кг",
            );
            state.fuel_mass = state.fuel_mass.min(state.aircraft_mass * 0.95);
            slider(
                ui,
                "Масса топлива",
                &mut state.fuel_mass,
                0.0..=state.aircraft_mass * 0.95,
                " кг",
            );
            ui.label("TSFC, кг/(Н·с)");
            ui.add(
                egui::DragValue::new(&mut state.tsfc)
                    .speed(0.000001)
                    .range(0.0000001..=0.01)
                    .max_decimals(7),
            );
            ui.add_space(10.0);
            ui.small("Винтовые двигатели: мощность оценивается из номинальной тяги. Запас батареи не моделируется. Дальность по Брегé доступна для ТРД и ТРДД.");
        }
    }
}
