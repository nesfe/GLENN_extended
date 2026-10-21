//! Individual workbench pages, separate from window composition and lifecycle.
use super::*;

impl Workbench {
    pub(super) fn active_page(&mut self, ui: &mut egui::Ui) {
        if self.page == 6 {
            controls::show(ui, &mut self.state, &mut self.control_tab);
            return;
        }
        self.telemetry(ui);
        ui.add_space(12.0);
        if self.analysis.current.mach > 0.75 {
            ui.colored_label(AMBER, "M > 0,75: Cp и силы — приближённые оценки.");
        }
        match self.page {
            0 => {
                let time = if self.view.animate {
                    ui.input(|input| input.time)
                } else {
                    0.0
                };
                tunnel::show(ui, &self.analysis, &mut self.view, time);
                if self.view.probe {
                    self.probe_ui(ui);
                }
            }
            1 => tunnel::section(ui, &self.analysis),
            2 => self.charts(ui),
            3 => self.flight_ui(ui),
            4 => self.files_ui(ui),
            _ => self.about_ui(ui),
        }
    }

    pub(super) fn telemetry(&self, ui: &mut egui::Ui) {
        let r = &self.analysis.current;
        ui.horizontal_wrapped(|ui| {
            for (label, value) in [
                ("ПОДЪЁМНАЯ СИЛА", format!("{:.2} кН", r.lift / 1000.0)),
                ("СОПРОТИВЛЕНИЕ", format!("{:.2} кН", r.drag / 1000.0)),
                ("КАЧЕСТВО L/D", format!("{:.2}", r.lift_drag_ratio())),
                ("ЧИСЛО МАХА", format!("{:.3}", r.mach)),
            ] {
                // Reserve the complete card before laying out its contents so
                // horizontal_wrapped knows when to move it to the next row.
                ui.allocate_ui_with_layout(
                    Vec2::new(150.0, 70.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::Frame::new()
                            .fill(Color32::from_rgb(21, 32, 46))
                            .corner_radius(8)
                            .inner_margin(12)
                            .show(ui, |ui| {
                                ui.set_min_width(126.0);
                                ui.label(RichText::new(label).size(10.0).color(MUTED));
                                ui.label(RichText::new(value).size(22.0).color(CYAN));
                            });
                    },
                );
            }
            ui.label(
                RichText::new(if r.stall_fraction > 0.4 {
                    "Срыв потока"
                } else if r.stall_fraction > 0.12 {
                    "Предсрывной режим"
                } else {
                    "Безотрывный режим"
                })
                .color(if r.stall_fraction > 0.12 { AMBER } else { CYAN }),
            );
        });
    }
    pub(super) fn charts(&self, ui: &mut egui::Ui) {
        ui.heading("Аэродинамические характеристики");
        ui.label("Поляра: −30…45° · одна геометрия, среда и скорость · точка — текущий режим");
        let polar: Vec<Point> = self
            .analysis
            .polar
            .iter()
            .map(|(_, r)| Point::new(r.cd, r.cl))
            .collect();
        let lift: Vec<Point> = self
            .analysis
            .polar
            .iter()
            .map(|(a, r)| Point::new(*a, r.cl))
            .collect();
        let drag: Vec<Point> = self
            .analysis
            .polar
            .iter()
            .map(|(a, r)| Point::new(*a, 5.0 * r.cd))
            .collect();
        let current = [Point::new(
            self.analysis.current.cd,
            self.analysis.current.cl,
        )];
        if ui.available_width() > 750.0 {
            ui.columns(2, |columns| {
                plots::plot(
                    &mut columns[0],
                    "Поляра",
                    "CD",
                    "CL",
                    &[("CL(CD)", &polar, CYAN), ("Режим", &current, AMBER)],
                    false,
                );
                plots::plot(
                    &mut columns[1],
                    "Угол атаки",
                    "α, °",
                    "CL / 5 CD",
                    &[("CL", &lift, CYAN), ("5 × CD", &drag, AMBER)],
                    false,
                );
            });
        } else {
            plots::plot(
                ui,
                "Поляра",
                "CD",
                "CL",
                &[("CL(CD)", &polar, CYAN), ("Режим", &current, AMBER)],
                false,
            );
            plots::plot(
                ui,
                "Угол атаки",
                "α, °",
                "CL / 5 CD",
                &[("CL", &lift, CYAN), ("5 × CD", &drag, AMBER)],
                false,
            );
        }
        plots::plot(
            ui,
            "Нагрузка по размаху",
            "y, м",
            "Н/м",
            &[(
                "Интеграл равен полной подъёмной силе",
                &self.analysis.current.span_loading,
                CYAN,
            )],
            false,
        );
        plots::plot(
            ui,
            "Циркуляция",
            "y, м",
            "м²/с",
            &[("Γ(y)", &self.analysis.current.circulation, AMBER)],
            false,
        );
    }
    pub(super) fn flight_ui(&self, ui: &mut egui::Ui) {
        ui.heading("Механика полёта");
        ui.label("Оценки установившегося полёта по параболической поляре. Максимальная скорость и потолок определяются дискретным поиском.");
        let p = &self.performance;
        let a = &self.analysis.atmosphere;
        let r = &self.analysis.current;
        let optional = |value: Option<f64>, unit: &str| {
            value.map_or_else(|| "Н/Д".into(), |v| format!("{v:.1} {unit}"))
        };
        let rows = [
            ("Скорость сваливания", format!("{:.1} м/с", p.stall_speed)),
            (
                "Наивыгоднейшая скорость",
                format!("{:.1} м/с", p.glide_speed),
            ),
            (
                "Минимальная мощность",
                format!("{:.1} м/с", p.endurance_speed),
            ),
            ("Максимальное качество", format!("{:.1}", p.glide_ratio)),
            ("Снижение при Vglide", format!("{:.2} м/с", p.sink_rate)),
            ("Располагаемая тяга", format!("{:.0} Н", p.thrust)),
            ("Требуемая тяга", format!("{:.0} Н", p.required_thrust)),
            (
                "Скороподъёмность сейчас",
                format!("{:.2} м/с", p.climb_rate),
            ),
            ("Угол набора", format!("{:.1}°", p.climb_angle)),
            (
                "Максимальная скорость · оценка",
                optional(p.maximum_speed, "м/с"),
            ),
            ("Практический потолок · Земля", optional(p.ceiling, "м")),
            ("Дальность · Брегé", optional(p.range_km, "км")),
            (
                "Продолжительность · Брегé",
                optional(p.endurance_hours, "ч"),
            ),
            ("Площадь крыла", format!("{:.2} м²", self.state.area())),
            ("Удлинение", format!("{:.2}", self.state.aspect_ratio())),
            ("Температура", format!("{:.2} K", a.temperature)),
            ("Давление", format!("{:.1} Па", a.pressure)),
            ("Плотность", format!("{:.5} кг/м³", a.density)),
            ("Число Рейнольдса", format!("{:.3e}", r.reynolds)),
            ("CL / CD", format!("{:.4} / {:.5}", r.cl, r.cd)),
            ("Момент c/4", format!("{:.1} Н·м", r.moment)),
        ];
        self.table(ui, "flight-table", &rows);
        if self.state.velocity < p.stall_speed {
            ui.colored_label(AMBER,"Скорость ниже расчётной скорости сваливания: установившийся горизонтальный полёт невозможен.");
        }
    }
    fn table(&self, ui: &mut egui::Ui, id: &str, rows: &[(&str, String)]) {
        let label_width = (ui.available_width() * 0.56).min(360.0);
        egui::Grid::new(id)
            .striped(true)
            .spacing([20.0, 12.0])
            .show(ui, |ui| {
                for (label, value) in rows {
                    ui.add_sized([label_width, 22.0], egui::Label::new(*label).wrap());
                    ui.label(RichText::new(value).color(CYAN));
                    ui.end_row();
                }
            });
    }
    pub(super) fn probe_ui(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.heading("Виртуальная трубка Пито");
        ui.horizontal_wrapped(|ui| {
            for (axis, label) in ["x/c", "y/c", "z/c"].iter().enumerate() {
                ui.add(
                    egui::Slider::new(&mut self.view.probe_position[axis], -5.0..=5.0).text(*label),
                );
            }
        });
        let probe = probe::sample(&self.analysis, self.view.probe_position);
        ui.label(format!(
            "V = {:.1} м/с   M = {:.3}   ps = {:.0} Па   pt = {:.0} Па   q = {:.0} Па",
            probe.speed,
            probe.mach,
            probe.static_pressure,
            probe.total_pressure,
            probe.dynamic_pressure
        ));
        ui.small("Локальное поле иллюстративное. Полное давление: изоэнтропическая формула / формула Рэлея за нормальным скачком.");
    }
    pub(super) fn files_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Проекты и обмен данными");
        ui.label(
            "Параметры автоматически сохраняются между запусками. Для переноса используйте JSON.",
        );
        if ui.button("Открыть DAT / DXF / JSON…").clicked() {
            self.import_dialog();
        }
        ui.add_space(16.0);
        for (extension, title, description) in [
            (
                "json",
                "Проект JSON",
                "Все физические параметры; поддерживается импорт проектов v2.",
            ),
            (
                "dat",
                "Профиль DAT",
                "Нормализованные координаты Selig: верхняя и нижняя поверхности.",
            ),
            (
                "csv",
                "Поляра CSV",
                "76 режимов по углу атаки с коэффициентами и силами.",
            ),
            (
                "obj",
                "Крыло OBJ",
                "Замкнутая треугольная сетка. Единицы — метры.",
            ),
            (
                "stl",
                "Крыло STL",
                "Та же замкнутая сетка. В слайсере задайте метры или масштаб ×1000 для мм.",
            ),
        ] {
            egui::Frame::group(ui.style())
                .inner_margin(12)
                .show(ui, |ui| {
                    if ui.button(title).clicked() {
                        self.export(extension);
                    }
                    ui.label(description);
                });
        }
        ui.add_space(12.0);
        ui.label("DAT: Selig и Lednicer с числом точек. DXF: одна POLYLINE или LWPOLYLINE из отрезков; сплайны и дуги предварительно дискретизируйте в CAD. Максимальный размер — 2 МБ.");
    }
    pub(super) fn about_ui(&self, ui: &mut egui::Ui) {
        ui.heading("GLENN Extended 3.0");
        ui.label("Нативная лаборатория аэродинамики на Rust. Автор проекта: nesfe. Лицензия MIT.");
        ui.hyperlink_to(
            "Исходный код и релизы",
            "https://github.com/nesfe/GLENN_extended",
        );
        ui.separator();
        for (title, text) in [
            (
                "Численное ядро",
                "Панели Гесса–Смита: постоянные источники, общая циркуляция и условие Кутты. Две базисные задачи на геометрию, затем линейная комбинация для каждого угла атаки.",
            ),
            (
                "Область применимости",
                "Потенциальное 2D-обтекание, инженерная поправка конечного крыла и эмпирический срыв. Это учебная модель; полноценные вязкие уравнения Навье–Стокса и трёхмерная несущая линия не решаются.",
            ),
            (
                "Ограничения",
                "Cp после срыва и при M > 0,75 не валидирован. Планетарные атмосферы приближённые. Предкрылки и закрылки моделируются деформацией без разрешения щелей. Траектории частиц и виртуальный зонд иллюстративные.",
            ),
            (
                "Что изменилось в v3",
                "Исправлены условие Кутты, разметка поверхностей Cp, давление ISA выше 20 км, формула Пито и единицы TSFC. Поэтому результаты намеренно отличаются от v2.",
            ),
            (
                "Управление",
                "Параметры доступны слева или на отдельной вкладке при ширине меньше 1000 точек. Каждая длинная панель прокручивается. Ctrl+O — открыть, Ctrl+S — сохранить. Масштаб интерфейса: Ctrl+ плюс / минус, Ctrl+0 — сброс.",
            ),
        ] {
            ui.label(RichText::new(title).strong().color(CYAN));
            ui.label(text);
            ui.add_space(12.0);
        }
    }
}
