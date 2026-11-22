//! Textos de la interfaz en 6 idiomas. Se usa el idioma de Windows (o SLIMCAST_LANG=en|es|pt|ja|zh|ko);
//! si no es ninguno de estos, inglés.
use std::fmt::Display;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Lang { En, Es, Pt, Ja, Zh, Ko }

unsafe extern "system" {
    fn GetUserDefaultUILanguage() -> u16; // kernel32
}

pub fn lang() -> Lang {
    static L: OnceLock<Lang> = OnceLock::new();
    *L.get_or_init(|| {
        let code = std::env::var("SLIMCAST_LANG").unwrap_or_default();
        match code.as_str() {
            "en" => return Lang::En, "es" => return Lang::Es, "pt" => return Lang::Pt,
            "ja" => return Lang::Ja, "zh" => return Lang::Zh, "ko" => return Lang::Ko,
            _ => {}
        }
        // los 10 bits bajos del LANGID son el idioma principal
        match unsafe { GetUserDefaultUILanguage() } & 0x3ff {
            0x0a => Lang::Es, 0x16 => Lang::Pt, 0x11 => Lang::Ja, 0x04 => Lang::Zh, 0x12 => Lang::Ko,
            _ => Lang::En,
        }
    })
}

/// Fuente de la interfaz: Segoe UI no tiene glifos CJK, y chino y japonés dibujan distinto los mismos caracteres.
pub fn font() -> &'static str {
    match lang() {
        Lang::Ja => "Yu Gothic UI",
        Lang::Zh => "Microsoft YaHei UI",
        Lang::Ko => "Malgun Gothic",
        _ => "Segoe UI",
    }
}

pub fn t(key: &str) -> &'static str {
    let row = TEXTS.iter().find(|r| r[0] == key).unwrap_or_else(|| panic!("falta el texto {key}"));
    row[lang() as usize + 1]
}

/// Como `t`, sustituyendo cada `{}` por el siguiente argumento.
pub fn tf(key: &str, args: &[&dyn Display]) -> String {
    let mut args = args.iter();
    let mut parts = t(key).split("{}");
    let mut s = parts.next().unwrap_or_default().to_string();
    for part in parts {
        s += &args.next().map(|a| a.to_string()).unwrap_or_default();
        s += part;
    }
    s
}

// clave, inglés, español, portugués, japonés, chino, coreano
const TEXTS: &[[&str; 7]] = &[
    ["add_videos", "Add videos", "Añadir videos", "Adicionar vídeos", "動画を追加", "添加视频", "동영상 추가"],
    ["add_folder", "Add folder", "Añadir carpeta", "Adicionar pasta", "フォルダーを追加", "添加文件夹", "폴더 추가"],
    ["disk", "Disk {}", "Disco {}", "Disco {}", "ディスク {}", "磁盘 {}", "디스크 {}"],
    ["free_of", "free of {}", "libres de {}", "livres de {}", "空き / 合計 {}", "可用 / 共 {}", "사용 가능 / 전체 {}"],
    ["freed", "+{} if you delete the originals", "+{} al borrar los originales", "+{} ao apagar os originais",
        "元ファイルを削除すると +{}", "删除原文件可释放 +{}", "원본을 삭제하면 +{}"],
    ["used", "Used", "Ocupado", "Ocupado", "使用中", "已用", "사용 중"],
    ["queued_size", "Queued {}", "En cola {}", "Na fila {}", "待機中 {}", "排队中 {}", "대기 중 {}"],
    ["freeable", "Freeable", "Liberable", "Liberável", "解放可能", "可释放", "확보 가능"],
    ["queued", "Queued", "En cola", "Na fila", "待機中", "排队中", "대기 중"],
    ["compressing", "Compressing {}%", "Comprimiendo {}%", "Comprimindo {}%", "圧縮中 {}%", "正在压缩 {}%", "압축 중 {}%"],
    ["done", "Done", "Listo", "Pronto", "完了", "完成", "완료"],
    ["cancelled", "Cancelled", "Cancelado", "Cancelado", "キャンセル済み", "已取消", "취소됨"],
    ["queue", "Queue", "Cola", "Fila", "キュー", "队列", "대기열"],
    ["count", "{}/{} videos", "{}/{} videos", "{}/{} vídeos", "{}/{} 本", "{}/{} 个视频", "{}/{}개 동영상"],
    ["note", "Originals are never touched. Output goes to the comprimidos folder",
        "Los originales no se tocan. El resultado va a la carpeta comprimidos",
        "Os originais não são alterados. O resultado vai para a pasta comprimidos",
        "元ファイルは変更されません。出力先は comprimidos フォルダーです",
        "原文件不会被改动，结果保存在 comprimidos 文件夹",
        "원본은 그대로 두고, 결과는 comprimidos 폴더에 저장됩니다"],
    ["drop", "Drop your recordings or a folder here", "Arrastra aquí tus grabaciones o una carpeta",
        "Arraste suas gravações ou uma pasta para cá", "録画ファイルかフォルダーをここにドロップ",
        "将录像或文件夹拖到这里", "녹화 파일이나 폴더를 여기에 끌어다 놓으세요"],
    ["q_best", "Best quality", "Máxima calidad", "Máxima qualidade", "最高画質", "最佳画质", "최고 화질"],
    ["q_balanced", "Balanced", "Equilibrado", "Equilibrado", "バランス", "均衡", "균형"],
    ["q_small", "Smallest", "Más pequeño", "Menor", "最小サイズ", "最小体积", "최소 용량"],
    ["detecting", "Detecting…", "Detectando…", "Detectando…", "検出中…", "正在检测…", "감지 중…"],
    ["fps30", "Lower to 30 fps", "Bajar a 30 fps", "Limitar a 30 fps", "30 fps に下げる", "降至 30 fps", "30fps로 낮추기"],
    ["cancel", "Cancel", "Cancelar", "Cancelar", "キャンセル", "取消", "취소"],
    ["compress", "Compress", "Comprimir", "Comprimir", "圧縮", "压缩", "압축"],
    ["compress_n", "Compress {} videos", "Comprimir {} videos", "Comprimir {} vídeos", "{} 本を圧縮", "压缩 {} 个视频", "동영상 {}개 압축"],
    ["device", "Device", "Dispositivo", "Dispositivo", "デバイス", "设备", "장치"],
    ["size", "Size", "Tamaño", "Tamanho", "サイズ", "大小", "크기"],
    ["no_ffmpeg_banner", "ffmpeg not found. Reinstall Slimcast or install ffmpeg with  winget install Gyan.FFmpeg --version 8.0.1",
        "No se encuentra ffmpeg. Reinstala Slimcast o instala ffmpeg con  winget install Gyan.FFmpeg --version 8.0.1",
        "ffmpeg não encontrado. Reinstale o Slimcast ou instale o ffmpeg com  winget install Gyan.FFmpeg --version 8.0.1",
        "ffmpeg が見つかりません。Slimcast を再インストールするか、次のコマンドで ffmpeg をインストールしてください  winget install Gyan.FFmpeg --version 8.0.1",
        "找不到 ffmpeg。请重新安装 Slimcast，或用以下命令安装 ffmpeg  winget install Gyan.FFmpeg --version 8.0.1",
        "ffmpeg를 찾을 수 없습니다. Slimcast를 다시 설치하거나 다음 명령으로 ffmpeg를 설치하세요  winget install Gyan.FFmpeg --version 8.0.1"],
    ["update_available", "Slimcast {} available", "Slimcast {} disponible", "Slimcast {} disponível",
        "Slimcast {} が利用可能です", "Slimcast {} 已发布", "Slimcast {} 사용 가능"],
    ["update_restart", "Slimcast will close and reopen updated", "Slimcast se cerrará y volverá a abrirse ya actualizado",
        "O Slimcast vai fechar e reabrir já atualizado", "Slimcast は終了し、更新後に再起動します",
        "Slimcast 将关闭，更新后自动重新打开", "Slimcast가 종료되고 업데이트 후 다시 열립니다"],
    ["update_later", "You can update when compression finishes", "Podrás actualizar al terminar la compresión",
        "Você poderá atualizar quando a compressão terminar", "圧縮が終わったら更新できます",
        "压缩完成后即可更新", "압축이 끝나면 업데이트할 수 있습니다"],
    ["downloading", "Downloading…", "Descargando…", "Baixando…", "ダウンロード中…", "正在下载…", "다운로드 중…"],
    ["update", "Update", "Actualizar", "Atualizar", "更新", "更新", "업데이트"],
    ["running_title", "A compression is running", "Hay una compresión en marcha", "Há uma compressão em andamento",
        "圧縮を実行中です", "正在进行压缩", "압축이 진행 중입니다"],
    ["running_detail", "If you quit, it will be cancelled and the partial file deleted",
        "Si sales se cancela y se borra el archivo a medias", "Se você sair, ela será cancelada e o arquivo incompleto apagado",
        "終了すると圧縮は中止され、途中のファイルは削除されます", "退出将取消压缩并删除未完成的文件",
        "종료하면 압축이 취소되고 미완성 파일이 삭제됩니다"],
    ["cancel_quit", "Cancel and quit", "Cancelar y salir", "Cancelar e sair", "中止して終了", "取消并退出", "취소하고 종료"],
    ["keep_going", "Keep going", "Seguir", "Continuar", "続ける", "继续", "계속"],
    ["already", "Already compressed", "Ya se comprimió antes", "Já foi comprimido", "圧縮済み", "已压缩过", "이미 압축됨"],
    ["no_space", "Not enough disk space", "Falta espacio en el disco", "Espaço insuficiente no disco",
        "ディスクの空き容量が不足しています", "磁盘空间不足", "디스크 공간이 부족합니다"],
    ["optimized", "Already optimized", "Ya está optimizado", "Já está otimizado", "最適化済み", "已是最优", "이미 최적화됨"],
    ["cant_read", "Couldn't read the video", "No se pudo leer el video", "Não foi possível ler o vídeo",
        "動画を読み込めません", "无法读取视频", "동영상을 읽을 수 없습니다"],
    ["cant_mkdir", "Couldn't create the comprimidos folder", "No se pudo crear la carpeta comprimidos",
        "Não foi possível criar a pasta comprimidos", "comprimidos フォルダーを作成できません",
        "无法创建 comprimidos 文件夹", "comprimidos 폴더를 만들 수 없습니다"],
    ["no_ffmpeg", "ffmpeg not found", "No se encuentra ffmpeg", "ffmpeg não encontrado", "ffmpeg が見つかりません",
        "找不到 ffmpeg", "ffmpeg를 찾을 수 없습니다"],
    ["ffmpeg_failed", "ffmpeg failed", "ffmpeg falló", "O ffmpeg falhou", "ffmpeg が失敗しました", "ffmpeg 运行失败", "ffmpeg 실행 실패"],
    ["duration", "Duration doesn't match ({}s of {}s)", "La duración no coincide ({}s de {}s)",
        "A duração não confere ({}s de {}s)", "長さが一致しません（{} 秒 / {} 秒）", "时长不一致（{} 秒 / {} 秒）",
        "길이가 일치하지 않습니다 ({}초 / {}초)"],
    ["cant_rename", "Couldn't rename the result", "No se pudo renombrar el resultado", "Não foi possível renomear o resultado",
        "出力ファイルの名前を変更できません", "无法重命名结果文件", "결과 파일 이름을 바꿀 수 없습니다"],
    ["download_failed", "Couldn't download the update", "No se pudo descargar la actualización",
        "Não foi possível baixar a atualização", "更新をダウンロードできません", "无法下载更新", "업데이트를 다운로드할 수 없습니다"],
    ["setup_failed", "Couldn't open the installer", "No se pudo abrir el instalador", "Não foi possível abrir o instalador",
        "インストーラーを開けません", "无法打开安装程序", "설치 프로그램을 열 수 없습니다"],
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_the_same_placeholders() {
        for row in TEXTS {
            let n = row[1].matches("{}").count();
            for text in &row[2..] {
                assert_eq!(text.matches("{}").count(), n, "{}: {text}", row[0]);
            }
        }
    }

    #[test]
    fn fills_placeholders_in_order() {
        assert!(tf("count", &[&3, &14]).starts_with("3/14"));
        assert!(tf("duration", &[&"12", &"30"]).contains("12") && tf("duration", &[&"12", &"30"]).contains("30"));
    }
}
