import { useState, useEffect, useCallback, useRef } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import type { DeviceInfo, DirEntry } from "../types";
import { browseDirectory, downloadFile, downloadDirectory, uploadFile as apiUploadFile, searchFiles as apiSearchFiles, getPairingCode } from "../api";
import PairingModal from "../components/PairingModal";

interface FileBrowserProps {
  device: DeviceInfo;
  downloadDir: string;
  onBatchDownload?: (count: number) => void;
  onStatusMessage: (msg: string) => void;
}

function fileIcon(name: string, isDir: boolean): string {
  if (isDir) return "📁";
  const ext = name.includes(".") ? name.split(".").pop()?.toLowerCase() : "";
  switch (ext) {
    case "jpg": case "jpeg": case "png": case "gif": case "bmp": case "webp": case "svg": case "ico":
      return "🖼️";
    case "mp4": case "mkv": case "avi": case "mov": case "wmv": case "flv": case "webm":
      return "🎬";
    case "mp3": case "wav": case "flac": case "aac": case "ogg": case "wma": case "m4a":
      return "🎵";
    case "zip": case "rar": case "7z": case "tar": case "gz": case "bz2": case "xz":
      return "🗜️";
    case "pdf":
      return "📕";
    case "doc": case "docx": case "xls": case "xlsx": case "ppt": case "pptx":
      return "📊";
    case "txt": case "md": case "log":
      return "📝";
    case "html": case "css": case "js": case "ts": case "tsx": case "jsx": case "json": case "xml": case "yaml": case "yml": case "toml":
    case "rs": case "py": case "go": case "java": case "c": case "cpp": case "h": case "hpp": case "rb": case "php": case "swift": case "kt":
      return "💻";
    case "exe": case "msi": case "dmg": case "app": case "deb": case "rpm":
      return "⚙️";
    default:
      return "📄";
  }
}

function formatFileSize(bytes: number | null): string {
  if (bytes === null) return "-";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function formatTime(timestamp: string | null): string {
  if (!timestamp) return "-";
  const d = new Date(parseInt(timestamp) * 1000);
  return d.toLocaleString();
}

const PAGE_SIZE = 100;

export default function FileBrowser({
  device,
  downloadDir,
  onBatchDownload,
  onStatusMessage,
}: FileBrowserProps) {
  const [entries, setEntries] = useState<DirEntry[]>([]);
  const [currentPath, setCurrentPath] = useState("");
  const [loading, setLoading] = useState(false);
  const [hasMore, setHasMore] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [selectedFiles, setSelectedFiles] = useState<Set<string>>(new Set());
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<DirEntry[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [dragOver, setDragOver] = useState(false);
  const [dragFile, setDragFile] = useState<DirEntry | null>(null);
  const [showPairing, setShowPairing] = useState(false);
  const browserRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const loadedRef = useRef(0); // 已加载条目数（用于分页偏移量）

  const loadDirectory = useCallback(
    async (path: string) => {
      setLoading(true);
      setHasMore(true);
      loadedRef.current = 0;
      try {
        const result = await browseDirectory(device.ip, device.port, path, 0, PAGE_SIZE);
        setEntries(result);
        loadedRef.current = result.length;
        setHasMore(result.length >= PAGE_SIZE);
        setCurrentPath(path);
      } catch (err) {
        if (String(err).includes("PAIRING_REQUIRED")) {
          setShowPairing(true);
        } else {
          onStatusMessage(`加载目录失败: ${err}`);
        }
      } finally {
        setLoading(false);
      }
    },
    [device.ip, device.port, onStatusMessage]
  );

  const loadMore = useCallback(async () => {
    if (loadingMore || !hasMore) return;
    setLoadingMore(true);
    try {
      const offset = loadedRef.current;
      const result = await browseDirectory(device.ip, device.port, currentPath, offset, PAGE_SIZE);
      setEntries(prev => [...prev, ...result]);
      loadedRef.current = offset + result.length;
      if (result.length < PAGE_SIZE) setHasMore(false);
    } catch (err) {
      onStatusMessage(`加载更多失败: ${err}`);
    } finally {
      setLoadingMore(false);
    }
  }, [device.ip, device.port, currentPath, loadingMore, hasMore, onStatusMessage]);

  useEffect(() => {
    loadDirectory("");
  }, [loadDirectory]);

  const handleNavigate = async (entry: DirEntry) => {
    if (entry.is_dir) {
      const newPath = currentPath
        ? `${currentPath}/${entry.name}`
        : entry.name;
      await loadDirectory(newPath);
    }
  };

  const handleGoUp = async () => {
    if (!currentPath) return;
    const parts = currentPath.split("/");
    parts.pop();
    await loadDirectory(parts.join("/"));
  };

  const handleDownload = async (entry: DirEntry) => {
    try {
      if (entry.is_dir) {
        const savePath = downloadDir
          ? `${downloadDir}/${entry.name}`
          : `${device.name}_${entry.name}`;
        const remotePath = currentPath ? `${currentPath}/${entry.name}` : entry.name;

        onStatusMessage(`开始下载文件夹: ${entry.name}`);
        await downloadDirectory(device.ip, device.port, remotePath, savePath);
        onStatusMessage(`已开始下载文件夹: ${entry.name}`);
      } else {
        const savePath = downloadDir
          ? `${downloadDir}/${entry.name}`
          : `${device.name}_${entry.name}`;
        const remotePath = currentPath ? `${currentPath}/${entry.name}` : entry.name;

        await downloadFile(
          device.ip,
          device.port,
          remotePath,
          savePath,
          entry.name,
          entry.size ?? 0
        );
        onStatusMessage(`已开始下载: ${entry.name}`);
      }
    } catch (err) {
      onStatusMessage(`下载失败: ${err}`);
    }
  };

  const toggleSelect = (name: string) => {
    setSelectedFiles((prev) => {
      const next = new Set(prev);
      if (next.has(name)) {
        next.delete(name);
      } else {
        next.add(name);
      }
      return next;
    });
  };

  const handleDownloadSelected = async () => {
    const count = selectedFiles.size;
    for (const name of selectedFiles) {
      const entry = entries.find((e) => e.name === name);
      if (entry && !entry.is_dir) {
        await handleDownload(entry);
      }
    }
    setSelectedFiles(new Set());
    if (count > 0 && onBatchDownload) {
      onBatchDownload(count);
    }
  };

  // 监听文件拖拽上传
  useEffect(() => {
    let cancelled = false;
    const unlistenPromise = listen<{ paths: string[] }>("tauri://drag-drop", async (event) => {
      if (cancelled) return;
      setDragOver(false);
      let uploadCount = 0;
      for (const path of event.payload.paths) {
        const name = path.split("/").pop() || path.split("\\").pop() || "file";
        onStatusMessage(`正在上传: ${name}`);
        try {
          await apiUploadFile(device.ip, device.port, currentPath, path);
          uploadCount++;
        } catch (err) {
          onStatusMessage(`上传失败: ${name} - ${err}`);
        }
      }
      if (uploadCount > 0) {
        onStatusMessage(`已上传 ${uploadCount} 个文件`);
      }
    });
    return () => {
      cancelled = true;
      unlistenPromise.then((fn) => fn());
    };
  }, [device.ip, device.port, currentPath, onStatusMessage]);

  // HTML5 drag-over 视觉反馈
  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(false);
  }, []);

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(false);
  }, []);

  // 拖拽下载（从文件列表拖出）
  const handleFileDragStart = (e: React.DragEvent, entry: DirEntry) => {
    e.dataTransfer.effectAllowed = 'copy';
    e.dataTransfer.setData('text/plain', entry.name);
    setDragFile(entry);
  };

  const handleFileDragEnd = () => {
    setDragFile(null);
  };

  const handleDownloadDrop = (e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (dragFile && !dragFile.is_dir) {
      handleDownload(dragFile);
    }
    setDragFile(null);
  };

  // 一键分享文件：生成分享链接并复制到剪贴板
  const handleShareFile = async (entry: DirEntry) => {
    try {
      const code = await getPairingCode();
      const relPath = currentPath ? `${currentPath}/${entry.name}` : entry.name;
      const shareUrl = `https://${device.ip}:${device.port}/s/${code}/${relPath}`;
      await navigator.clipboard.writeText(shareUrl);
      onStatusMessage(`分享链接已复制: ${entry.name}`);
    } catch (err) {
      onStatusMessage(`生成分享链接失败: ${err}`);
    }
  };

  let searchTimer: ReturnType<typeof setTimeout>;

  const handleSearch = (value: string) => {
    setSearchQuery(value);
    clearTimeout(searchTimer);
    if (!value.trim()) {
      setSearchResults(null);
      return;
    }
    searchTimer = setTimeout(async () => {
      setSearching(true);
      try {
        const results = await apiSearchFiles(device.ip, device.port, currentPath, value.trim());
        setSearchResults(results);
      } catch (err) {
        setSearchResults([]);
      } finally {
        setSearching(false);
      }
    }, 300);
  };

  const handleUpload = async () => {
    try {
      const selected = await open({
        multiple: false,
        title: "选择要上传的文件",
      });
      if (!selected) return;
      const path = selected as string;
      const name = path.split("/").pop() || path.split("\\").pop() || "file";
      onStatusMessage(`正在上传: ${name}`);
      await apiUploadFile(device.ip, device.port, currentPath, path);
      onStatusMessage(`已开始上传: ${name}`);
    } catch (err) {
      onStatusMessage(`上传失败: ${err}`);
    }
  };

  // 键盘快捷键
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const meta = e.metaKey || e.ctrlKey;
      if (meta && e.key === "f") {
        e.preventDefault();
        searchRef.current?.focus();
      } else if (meta && e.key === "u") {
        e.preventDefault();
        handleUpload();
      } else if (e.key === "Escape") {
        setSearchQuery("");
        setSearchResults(null);
        setSelectedFiles(new Set());
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  });

  return (
    <div
      className={`file-browser${dragOver ? " drag-over" : ""}`}
      ref={browserRef}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      {dragOver && !dragFile && (
        <div className="drag-overlay">
          <span>📂 释放以上传文件</span>
        </div>
      )}
      {dragFile && !dragFile.is_dir && (
        <div
          className="download-drop-zone"
          onDragOver={(e) => { e.preventDefault(); e.stopPropagation(); }}
          onDrop={handleDownloadDrop}
          onDragLeave={() => {}}
        >
          <span>📥 释放以下载: {dragFile.name}</span>
        </div>
      )}
      <div className="page-header">
        <h2>
          浏览: {device.name} ({device.ip})
        </h2>
        <div className="path-bar">
          <button className="btn btn-sm" onClick={handleGoUp} disabled={!currentPath}>
            ⬆ 上级目录
          </button>
          <button className="btn btn-sm btn-primary" onClick={handleUpload}>
            ⬆ 上传
          </button>
          <span className="current-path">/{currentPath}</span>
        </div>
      </div>

      {selectedFiles.size > 0 && (
        <div className="batch-bar">
          <span>已选择 {selectedFiles.size} 项</span>
          <button className="btn btn-sm btn-primary" onClick={handleDownloadSelected}>
            下载选中
          </button>
          <button
            className="btn btn-sm"
            onClick={() => setSelectedFiles(new Set())}
          >
            取消
          </button>
        </div>
      )}

      <div className="search-bar">
        <input
          ref={searchRef}
          type="text"
          className="search-input"
          placeholder="搜索文件名..."
          value={searchQuery}
          onChange={(e) => handleSearch(e.target.value)}
        />
        {searching && <span className="search-spinner" />}
        {searchResults !== null && (
          <span className="search-count">
            找到 {searchResults.length} 个结果
            <button className="btn btn-xs" onClick={() => { setSearchQuery(""); setSearchResults(null); }}>
              清除
            </button>
          </span>
        )}
      </div>

      {searchResults !== null ? (
        searchResults.length === 0 ? (
          <div className="empty-state"><p>未找到匹配的文件</p></div>
        ) : (
          <div className="file-table-wrapper">
          <table className="file-table">
            <thead>
              <tr>
                <th className="col-name">名称</th>
                <th className="col-size">大小</th>
                <th className="col-time">修改时间</th>
              </tr>
            </thead>
            <tbody>
              {searchResults.map((entry) => (
                <tr key={entry.name} className={entry.is_dir ? "row-dir" : "row-file"}>
                  <td className="cell-name">
                    <span className="file-icon">{fileIcon(entry.name, entry.is_dir)}</span>
                    {entry.name}
                  </td>
                  <td className="cell-size">{formatFileSize(entry.size)}</td>
                  <td className="cell-time">{formatTime(entry.modified_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          </div>
        )
      ) : loading ? (
        <div className="loading">
          <div className="spinner" />
          <span>加载中...</span>
        </div>
      ) : entries.length === 0 ? (
        <div className="empty-state">
          <p>目录为空</p>
        </div>
      ) : (
        <>
        <div className="file-table-wrapper">
        <table className="file-table">
          <thead>
            <tr>
              <th className="col-select"></th>
              <th className="col-name">名称</th>
              <th className="col-size">大小</th>
              <th className="col-time">修改时间</th>
              <th className="col-action">操作</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((entry) => (
              <tr
                key={entry.name}
                className={entry.is_dir ? "row-dir" : "row-file"}
                draggable={!entry.is_dir}
                onDoubleClick={() => handleNavigate(entry)}
                onDragStart={(e) => handleFileDragStart(e, entry)}
                onDragEnd={handleFileDragEnd}
              >
                <td>
                  <input
                    type="checkbox"
                    checked={selectedFiles.has(entry.name)}
                    onChange={() => toggleSelect(entry.name)}
                    onClick={(e) => e.stopPropagation()}
                  />
                </td>
                <td
                  className="cell-name"
                  onClick={() => handleNavigate(entry)}
                  role="button"
                  tabIndex={0}
                  onKeyDown={(e) =>
                    e.key === "Enter" && handleNavigate(entry)
                  }
                >
                  <span className="file-icon">
                    {fileIcon(entry.name, entry.is_dir)}
                  </span>
                  {entry.name}
                </td>
                <td className="cell-size">{formatFileSize(entry.size)}</td>
                <td className="cell-time">{formatTime(entry.modified_at)}</td>
                <td>
                  <div style={{ display: "flex", gap: 4 }}>
                  {!entry.is_dir && (
                    <>
                    <button
                      className="btn btn-xs btn-primary"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleDownload(entry);
                      }}
                    >
                      下载
                    </button>
                    <button
                      className="btn btn-xs"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleShareFile(entry);
                      }}
                      title="复制分享链接到剪贴板"
                    >
                      分享
                    </button>
                    </>
                  )}
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
        </div>
        {hasMore && (
          <div style={{ textAlign: "center", padding: "12px" }}>
            <button className="btn" onClick={loadMore} disabled={loadingMore}>
              {loadingMore ? "加载中..." : `加载更多 (已加载 ${entries.length}+)`}
            </button>
          </div>
        )}
        </>
      )}

      {showPairing && (
        <PairingModal
          deviceIp={device.ip}
          port={device.port}
          onClose={() => setShowPairing(false)}
          onSuccess={() => {
            setShowPairing(false);
            onStatusMessage("配对成功");
            loadDirectory(currentPath);
          }}
        />
      )}
    </div>
  );
}
