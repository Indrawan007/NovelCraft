//! Daily net saved word changes for chapters in a project.
use iced::widget::{button, column, row, scrollable, text, text_input, Column};
use iced::{Element, Length, Task};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Day { pub date: String, pub net: i64 }
#[derive(Debug, Clone)]
pub struct Report { pub target: Option<u32>, pub days: Vec<Day>, pub streak: usize }

#[derive(Default)]
pub struct Activity {
    folder: Option<PathBuf>, report: Option<Report>, busy: bool,
    target_input: String, target_baseline: String, notice: Option<String>,
}
#[derive(Debug, Clone)]
pub enum Message {
    Load(PathBuf), Loaded(Result<Report, String>),
    TargetChanged(String), SaveTarget, CancelTarget, TargetSaved(Result<Report, String>),
}
impl Activity {
    pub fn is_busy(&self) -> bool { self.busy }
    pub fn is_dirty(&self) -> bool { self.target_input != self.target_baseline }
    pub fn apply_recorded(&mut self, folder: PathBuf, report: Report) {
        self.folder = Some(folder);
        self.apply_report(report);
    }
    fn apply_report(&mut self, report: Report) {
        if !self.is_dirty() {
            self.target_input = report.target.map(|n| n.to_string()).unwrap_or_default();
            self.target_baseline = self.target_input.clone();
        }
        self.report = Some(report);
    }
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Load(folder) if !self.busy && !self.is_dirty() => {
                self.busy = true; self.folder = Some(folder.clone()); self.notice = None;
                return Task::perform(load_async(folder), Message::Loaded);
            }
            Message::Load(_) if self.is_dirty() => self.notice = Some("Simpan atau batalkan target sebelum memuat ulang.".into()),
            Message::Loaded(result) => {
                self.busy = false;
                match result { Ok(report) => { self.apply_report(report); self.notice = None; }, Err(e) => self.notice = Some(e) }
            }
            Message::TargetChanged(value) if !self.busy && self.report.is_some() => self.target_input = value,
            Message::CancelTarget if !self.busy => { self.target_input = self.target_baseline.clone(); self.notice = None; }
            Message::SaveTarget if !self.busy && self.report.is_some() => {
                if let Some(folder) = self.folder.clone() {
                    let desired = match parse_target(&self.target_input) { Ok(target) => target, Err(e) => { self.notice = Some(e); return Task::none(); } };
                    let expected = match parse_target(&self.target_baseline) { Ok(target) => target, Err(e) => { self.notice = Some(e); return Task::none(); } };
                    self.busy = true; self.notice = None;
                    return Task::perform(save_target_async(folder, expected, desired), Message::TargetSaved);
                }
            }
            Message::TargetSaved(result) => {
                self.busy = false;
                match result {
                    Ok(report) => {
                        self.target_input = report.target.map(|n| n.to_string()).unwrap_or_default();
                        self.target_baseline = self.target_input.clone(); self.report = Some(report); self.notice = None;
                    }
                    Err(e) => self.notice = Some(e),
                }
            }
            _ => {}
        }
        Task::none()
    }
    pub fn view(&self) -> Element<'_, Message> {
        let mut content = column![text("Target dan streak harian").size(20)].spacing(8);
        if let Some(notice) = &self.notice { content = content.push(text(notice).size(13)); }
        if self.busy { content = content.push(text("Memproses statistik...")); }
        if let Some(folder) = &self.folder {
            let mut reload = button("Muat ulang statistik");
            if !self.busy { reload = reload.on_press(Message::Load(folder.clone())); }
            content = content.push(reload);
        }
        if let Some(report) = &self.report {
            let today = report.days.last().map_or(0, |d| d.net.max(0));
            let target = report.target.map_or_else(|| "belum ditetapkan".to_owned(), |n| format!("{n} kata"));
            let date = report.days.last().map_or("tanggal tidak tersedia", |day| day.date.as_str());
            content = content.push(text(format!("Tanggal laporan {date}: {today} kata bersih · Target: {target} · Streak: {} hari", report.streak)));
            content = content.push(text("Hitungan berdasarkan perubahan bersih saat chapter tersimpan, menurut tanggal lokal.").size(12));
            let mut input = text_input("Target harian (kosongkan untuk menghapus)", &self.target_input).width(250);
            let mut save = button("Simpan target"); let mut cancel = button("Batalkan target");
            if !self.busy { input = input.on_input(Message::TargetChanged); save = save.on_press(Message::SaveTarget); cancel = cancel.on_press(Message::CancelTarget); }
            content = content.push(row![input, save, cancel].spacing(8).wrap());
            let mut days = Column::new().spacing(4);
            for day in report.days.iter().rev() {
                days = days.push(text(format!("{}   {} kata bersih", day.date, day.net.max(0))).size(13));
            }
            content = content.push(text("Kalender — 31 hari terakhir").size(16)).push(days);
        }
        scrollable(content).height(Length::Fill).into()
    }
}
fn parse_target(input: &str) -> Result<Option<u32>, String> {
    if input.is_empty() { return Ok(None); }
    if !input.bytes().all(|b| b.is_ascii_digit()) { return Err("Target harus berupa angka 1–1.000.000.000 atau kosong.".into()); }
    input.parse::<u32>().ok().filter(|n| (1..=1_000_000_000).contains(n)).map(Some)
        .ok_or_else(|| "Target harus berupa angka 1–1.000.000.000 atau kosong.".into())
}
fn streak(db: &Connection, today: &str) -> Result<usize, String> {
    let count: i64 = db.query_row("WITH RECURSIVE chain(day,n) AS (SELECT CASE WHEN EXISTS (SELECT 1 FROM writing_days WHERE day=?1 AND net_words>0) THEN ?1 ELSE date(?1,'-1 day') END, 0 UNION ALL SELECT date(chain.day,'-1 day'), chain.n+1 FROM chain WHERE EXISTS (SELECT 1 FROM writing_days WHERE writing_days.day=chain.day AND net_words>0)) SELECT MAX(n) FROM chain", params![today], |row| row.get(0))
        .map_err(|e| format!("Gagal menghitung streak: {e}"))?;
    usize::try_from(count).map_err(|_| "Streak tidak valid.".into())
}
fn report(db: &Connection) -> Result<Report, String> {
    let today: String = db.query_row("SELECT date('now','localtime')", [], |row| row.get(0)).map_err(|e| format!("Gagal membaca tanggal lokal: {e}"))?;
    report_at(db, &today)
}
fn report_at(db: &Connection, today: &str) -> Result<Report, String> {
    let target: Option<i64> = db.query_row("SELECT value FROM writing_settings WHERE key='daily_target'", [], |r| r.get(0)).optional().map_err(|e| format!("Gagal membaca target: {e}"))?;
    let target = target.map(|n| u32::try_from(n).ok().filter(|n| (1..=1_000_000_000).contains(n)).ok_or("Target tersimpan tidak valid.".to_owned())).transpose()?;
    let mut stmt = db.prepare("WITH RECURSIVE days(day) AS (SELECT date(?1,'-30 days') UNION ALL SELECT date(day,'+1 day') FROM days WHERE day < ?1) SELECT days.day, COALESCE(writing_days.net_words,0) FROM days LEFT JOIN writing_days ON days.day=writing_days.day ORDER BY days.day").map_err(|e| format!("Gagal membaca kalender: {e}"))?;
    let days = stmt.query_map(params![today], |row| Ok(Day { date: row.get(0)?, net: row.get(1)? })).map_err(|e| format!("Gagal membaca kalender: {e}"))?.collect::<Result<Vec<_>, _>>().map_err(|e| format!("Gagal membaca kalender: {e}"))?;
    let streak = streak(db, today)?;
    Ok(Report { target, days, streak })
}
fn load(folder: &Path) -> Result<Report, String> { report(&crate::reference::connection(folder)?) }
async fn load_async(folder: PathBuf) -> Result<Report, String> {
    tokio::task::spawn_blocking(move || load(&folder)).await.map_err(|e| format!("Proses statistik gagal: {e}"))?
}
fn save_target(folder: &Path, expected: Option<u32>, desired: Option<u32>) -> Result<Report, String> {
    let mut db = crate::reference::connection(folder)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| format!("Gagal menyimpan target: {e}"))?;
    let current: Option<i64> = tx.query_row("SELECT value FROM writing_settings WHERE key='daily_target'", [], |r| r.get(0)).optional().map_err(|e| format!("Gagal membaca target: {e}"))?;
    if current != expected.map(i64::from) { return Err("Target berubah di luar aplikasi; batalkan perubahan dan muat ulang statistik.".into()); }
    match desired {
        Some(value) => { tx.execute("INSERT INTO writing_settings(key,value) VALUES('daily_target',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![value]).map_err(|e| format!("Gagal menyimpan target: {e}"))?; }
        None => { tx.execute("DELETE FROM writing_settings WHERE key='daily_target'", []).map_err(|e| format!("Gagal menghapus target: {e}"))?; }
    }
    let result = report(&tx)?;
    tx.commit().map_err(|e| format!("Gagal menyimpan target: {e}"))?;
    Ok(result)
}
async fn save_target_async(folder: PathBuf, expected: Option<u32>, desired: Option<u32>) -> Result<Report, String> {
    tokio::task::spawn_blocking(move || save_target(&folder, expected, desired)).await.map_err(|e| format!("Proses statistik gagal: {e}"))?
}
pub async fn record_async(folder: PathBuf, delta: i64) -> Result<Report, String> {
    tokio::task::spawn_blocking(move || record(&folder, delta)).await.map_err(|e| format!("Proses statistik gagal: {e}"))?
}
fn record(folder: &Path, delta: i64) -> Result<Report, String> {
    let mut db = crate::reference::connection(folder)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate).map_err(|e| format!("Gagal mencatat statistik: {e}"))?;
    let today: String = tx.query_row("SELECT date('now','localtime')", [], |row| row.get(0)).map_err(|e| format!("Gagal membaca tanggal lokal: {e}"))?;
    tx.execute("INSERT INTO writing_days(day,net_words) VALUES(?1,?2) ON CONFLICT(day) DO UPDATE SET net_words=net_words+excluded.net_words", params![today, delta]).map_err(|e| format!("Gagal mencatat statistik: {e}"))?;
    let result = report_at(&tx, &today)?;
    tx.commit().map_err(|e| format!("Gagal mencatat statistik: {e}"))?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    fn dir() -> PathBuf { static N: AtomicU64 = AtomicU64::new(0); std::env::temp_dir().join(format!("novelcraft-activity-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))) }
    #[test]
    fn unsaved_target_survives_a_recorded_report() {
        let dir = dir(); std::fs::create_dir_all(&dir).unwrap();
        let mut activity = Activity::default();
        let _ = activity.update(Message::Loaded(load(&dir)));
        let _ = activity.update(Message::TargetChanged("500".into()));
        assert!(activity.is_dirty());
        activity.apply_recorded(dir.clone(), record(&dir, 3).unwrap());
        assert_eq!(activity.target_input, "500");
        assert!(activity.is_dirty());
        let _ = activity.update(Message::Load(dir.clone()));
        assert!(!activity.is_busy());
        let _ = activity.update(Message::CancelTarget);
        assert!(!activity.is_dirty());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn target_and_net_activity_persist_and_conflicts_are_rejected() {
        let dir = dir(); std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(load(&dir).unwrap().target, None);
        let report = save_target(&dir, None, Some(500)).unwrap(); assert_eq!(report.target, Some(500));
        assert!(save_target(&dir, None, Some(100)).is_err());
        record(&dir, 10).unwrap(); record(&dir, -3).unwrap();
        let report = load(&dir).unwrap(); assert_eq!(report.days.len(), 31); assert_eq!(report.days.last().unwrap().net, 7); assert_eq!(report.streak, 1);
        record(&dir, -10).unwrap(); assert_eq!(load(&dir).unwrap().streak, 0);
        let report = save_target(&dir, Some(500), None).unwrap(); assert_eq!(report.target, None);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn streak_handles_long_runs_today_yesterday_and_gaps() {
        let dir = dir(); std::fs::create_dir_all(&dir).unwrap();
        let db = crate::reference::connection(&dir).unwrap();
        db.execute_batch("WITH RECURSIVE d(day) AS (SELECT date('2025-01-01') UNION ALL SELECT date(day,'+1 day') FROM d WHERE day < '2025-02-10') INSERT INTO writing_days SELECT day,1 FROM d;").unwrap();
        assert_eq!(streak(&db, "2025-02-10").unwrap(), 41);
        assert_eq!(streak(&db, "2025-02-11").unwrap(), 41);
        assert_eq!(streak(&db, "2025-02-12").unwrap(), 0);
        db.execute("UPDATE writing_days SET net_words=0 WHERE day='2025-02-08'", []).unwrap();
        assert_eq!(streak(&db, "2025-02-10").unwrap(), 2);
        assert!(parse_target("0").is_err()); assert!(parse_target("1.5").is_err());
        assert_eq!(parse_target(""), Ok(None));
        drop(db); std::fs::remove_dir_all(dir).unwrap();
    }
}
