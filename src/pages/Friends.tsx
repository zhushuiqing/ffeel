import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { PairedDeviceInfo, ChatEntry } from "../types";
import * as api from "../api";

interface FriendsProps {
  trustedDeviceList: PairedDeviceInfo[];
  onlineDeviceIds: string[];
  messages: ChatEntry[];
  onMessagesUpdate?: (msgs: ChatEntry[]) => void;
  onStatusMessage?: (msg: string) => void;
}

// macOS-style color palette for avatars
const AVATAR_COLORS = [
  "#FF6B6B", "#FFA94D", "#FFD43B", "#69DB7C",
  "#38D9A9", "#4DABF7", "#748FFC", "#DA77F2",
  "#F06595", "#FF922B",
];

function getAvatarColor(name: string): string {
  let hash = 0;
  for (let i = 0; i < name.length; i++) {
    hash = name.charCodeAt(i) + ((hash << 5) - hash);
  }
  return AVATAR_COLORS[Math.abs(hash) % AVATAR_COLORS.length];
}

function getInitial(name: string): string {
  return name.charAt(0).toUpperCase();
}

function formatDateSeparator(ts: string): string {
  try {
    const d = new Date(ts);
    const now = new Date();
    const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
    const yesterday = new Date(today);
    yesterday.setDate(yesterday.getDate() - 1);

    if (d >= today) return "今天";
    if (d >= yesterday) return "昨天";

    const month = d.getMonth() + 1;
    const day = d.getDate();
    return `${month}月${day}日`;
  } catch {
    return ts;
  }
}

function formatTime(ts: string): string {
  try {
    const d = new Date(ts);
    return d.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" });
  } catch {
    return ts;
  }
}

function formatFileSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const k = 1024;
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + units[i];
}

// Simple emoji list
const EMOJI_LIST = [
  "😀", "😃", "😄", "😁", "😅", "😂", "🤣", "😊",
  "😇", "🙂", "😉", "😌", "😍", "🥰", "😘", "😗",
  "😋", "😛", "😜", "🤪", "😝", "🤑", "🤗", "🤭",
  "🤔", "🤐", "😐", "😑", "😶", "😏", "😒", "🙄",
  "😬", "😮", "😯", "😲", "😳", "🥺", "😢", "😭",
  "😤", "😠", "😡", "🤬", "😈", "👿", "💀", "☠️",
  "💩", "🤡", "👹", "👺", "👻", "👽", "🤖", "🎃",
  "👍", "👎", "👊", "✊", "🤛", "🤜", "👏", "🙌",
  "🤝", "💪", "✌️", "🤞", "🖕", "🤟", "🤘", "👌",
  "❤️", "🧡", "💛", "💚", "💙", "💜", "🖤", "🤍",
  "💯", "🔥", "⭐", "✨", "💥", "🌈", "☀️", "🌙",
];

// Image component that loads via Tauri command for desktop
function ChatImage({ fileId, fileName }: { fileId: string; fileName?: string }) {
  const [src, setSrc] = useState<string>("");
  useEffect(() => {
    api.getChatImageBase64(fileId).then(setSrc).catch(() => {});
  }, [fileId]);
  if (!src) return <div className="image-thumb" style={{ background: "var(--bg-content)", minHeight: 60, display: "flex", alignItems: "center", justifyContent: "center", fontSize: 11, color: "var(--text-tertiary)" }}>加载中...</div>;
  return <img src={src} alt={fileName || "图片"} loading="lazy" style={{ width: "100%", height: "100%", objectFit: "cover", display: "block" }} />;
}

export default function Friends({
  trustedDeviceList,
  onlineDeviceIds,
  messages,
  onMessagesUpdate,
  onStatusMessage,
}: FriendsProps) {
  const [inputText, setInputText] = useState("");
  const [showEmojiPicker, setShowEmojiPicker] = useState(false);
  const [previewImage, setPreviewImage] = useState<string | null>(null);
  const [selectedFriend, setSelectedFriend] = useState<PairedDeviceInfo | null>(null);
  const [fileStates, setFileStates] = useState<Record<string, { status: 'idle' | 'downloading' | 'done', savedPath?: string }>>({});
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const emojiPickerRef = useRef<HTMLDivElement>(null);

  // Auto-scroll to bottom
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  // Close emoji picker on outside click
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (emojiPickerRef.current && !emojiPickerRef.current.contains(e.target as Node)) {
        setShowEmojiPicker(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, []);

  const handleSend = async () => {
    const text = inputText.trim();
    if (!text || !selectedFriend) return;
    try {
      await api.sendChatMessage(text, selectedFriend.id);
      setInputText("");
      const msgs = await api.getChatMessages();
      onMessagesUpdate?.(msgs);
    } catch (err) {
      onStatusMessage?.(`发送失败: ${err}`);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const handleEmojiClick = (emoji: string) => {
    setInputText(prev => prev + emoji);
    setShowEmojiPicker(false);
  };

  const handleFileSelect = async () => {
    try {
      const selected = await open({
        multiple: false,
        title: "选择要发送的文件",
      });
      if (!selected) return;
      const filePath = selected as string;
      const fileName = filePath.split('/').pop() || filePath.split('\\').pop() || 'file';
      await api.sendChatFile(filePath, selectedFriend?.id);
      onStatusMessage?.(`已发送文件: ${fileName}`);
    } catch (err) {
      onStatusMessage?.(`发送文件失败: ${err}`);
    }
  };

  const getFileIcon = (fileType?: string): string => {
    if (!fileType) return "📄";
    if (fileType.startsWith("image/")) return "🖼️";
    if (fileType.startsWith("video/")) return "🎬";
    if (fileType.startsWith("audio/")) return "🎵";
    if (fileType.includes("pdf")) return "📕";
    if (fileType.includes("zip") || fileType.includes("rar") || fileType.includes("tar")) return "📦";
    if (fileType.includes("text") || fileType.includes("document")) return "📝";
    return "📄";
  };

  const handleImageClick = async (chatEntry: ChatEntry) => {
    if (chatEntry.file_id && chatEntry.message_type === "image") {
      try {
        const dataUrl = await api.getChatImageBase64(chatEntry.file_id);
        setPreviewImage(dataUrl);
      } catch {
        // fallback to HTTP URL if base64 fails
        setPreviewImage(`/api/chat/download/${chatEntry.file_id}`);
      }
    }
  };

  const handleFileClick = async (entry: ChatEntry) => {
    if (!entry.file_id) return;
    const cur = fileStates[entry.file_id];
    if (cur?.status === 'done' && cur.savedPath) {
      // 已下载，双击打开
      try {
        await api.openFile(cur.savedPath);
      } catch (err) {
        onStatusMessage?.(`打开文件失败: ${err}`);
      }
      return;
    }
    if (cur?.status === 'downloading') return;
    // 开始下载
    setFileStates(prev => ({ ...prev, [entry.file_id!]: { status: 'downloading' } }));
    try {
      const savedPath = await api.downloadChatFile(entry.file_id);
      setFileStates(prev => ({ ...prev, [entry.file_id!]: { status: 'done', savedPath } }));
      onStatusMessage?.(`已保存到: ${savedPath}`);
    } catch (err) {
      setFileStates(prev => ({ ...prev, [entry.file_id!]: { status: 'idle' } }));
      onStatusMessage?.(`下载失败: ${err}`);
    }
  };

  // Filter messages that involve the selected friend (from or to)
  const friendMessages = selectedFriend
    ? messages.filter(msg => msg.from_id === selectedFriend.id || msg.to_id === selectedFriend.id)
    : [];

  // Group messages by date
  const getDateKey = (ts: string): string => {
    try {
      const d = new Date(ts);
      return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
    } catch {
      return ts;
    }
  };

  const groups: { dateKey: string; messages: ChatEntry[] }[] = [];
  let currentGroup: { dateKey: string; messages: ChatEntry[] } | null = null;
  for (const msg of friendMessages) {
    const key = getDateKey(msg.timestamp);
    if (!currentGroup || currentGroup.dateKey !== key) {
      currentGroup = { dateKey: key, messages: [] };
      groups.push(currentGroup);
    }
    currentGroup.messages.push(msg);
  }

  const handleSelectFriend = (info: PairedDeviceInfo) => {
    setSelectedFriend(prev => prev?.id === info.id ? null : info);
  };

  return (
    <div className="friends-page">
      {/* Paired Friends Sidebar */}
      {trustedDeviceList.length > 0 && (
        <div className="friends-sidebar">
          <h3 className="friends-title">好友 ({trustedDeviceList.length})</h3>
          <div className="friends-list">
            {trustedDeviceList.map((info) => {
              const isOnline = onlineDeviceIds.includes(info.id);
              const isSelected = selectedFriend?.id === info.id;
              return (
                <div
                  key={info.id}
                  className={`friend-item ${isSelected ? "selected" : ""}`}
                  onClick={() => handleSelectFriend(info)}
                >
                  <div className="friend-avatar-wrapper">
                    <div
                      className="friend-avatar"
                      style={{ backgroundColor: getAvatarColor(info.nickname || info.name) }}
                    >
                      {getInitial(info.nickname || info.name)}
                    </div>
                    <span className={`friend-status-dot ${isOnline ? "online" : "offline"}`} />
                  </div>
                  <div className="friend-info">
                    <div className="friend-name">
                      {info.nickname || info.name}
                    </div>
                    <div className="friend-id">
                      {isOnline ? "在线" : "离线"}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Chat Area */}
      {selectedFriend ? (
      <div className="chat-area">
        <div className="chat-header">
          <h3>
            <span
              className="chat-avatar"
              style={{ backgroundColor: getAvatarColor(selectedFriend.nickname || selectedFriend.name), width: 24, height: 24, fontSize: 10, display: "inline-flex", verticalAlign: "middle", marginRight: 6 }}
            >
              {getInitial(selectedFriend.nickname || selectedFriend.name)}
            </span>
            {selectedFriend.nickname || selectedFriend.name}
          </h3>
        </div>

        <div className="chat-messages">
          {friendMessages.length === 0 && (
            <div className="chat-empty">
              <p>暂无消息</p>
              <p className="hint">发送第一条消息开始聊天吧</p>
            </div>
          )}

          {groups.map((group) => (
            <div key={group.dateKey} className="chat-date-group">
              <div className="chat-date-separator">
                <span>{formatDateSeparator(group.messages[0].timestamp)}</span>
              </div>

              {group.messages.map((msg, i) => {
                const paired = trustedDeviceList.find((d) => d.id === msg.from_id);
                const displayName = paired?.nickname || paired?.name || msg.from_name;
                const isSelf = msg.from_id !== selectedFriend?.id;

                return (
                  <div
                    key={`${group.dateKey}-${i}`}
                    className={`chat-msg ${isSelf ? "chat-msg-self" : "chat-msg-other"}`}
                  >
                    <div
                      className="chat-avatar"
                      style={{ backgroundColor: getAvatarColor(displayName) }}
                    >
                      {getInitial(displayName)}
                    </div>

                    <div className="chat-msg-body">
                      <div className="chat-msg-sender">{displayName}</div>

                      {/* Text message */}
                      {msg.message_type === "text" && (
                        <div className="chat-msg-bubble">{msg.text}</div>
                      )}

                      {/* File message */}
                      {msg.message_type === "file" && msg.file_name && msg.file_id && (
                        <div
                          className={`file-card ${fileStates[msg.file_id]?.status === 'done' ? 'file-card-done' : ''}`}
                          onDoubleClick={() => handleFileClick(msg)}
                          onClick={() => {
                            const st = fileStates[msg.file_id!];
                            if (!st || st.status === 'idle') handleFileClick(msg);
                          }}
                        >
                          <span className="file-card-icon">{getFileIcon(msg.file_type)}</span>
                          <div className="file-card-info">
                            <div className="file-card-name">{msg.file_name}</div>
                            {msg.file_size != null && (
                              <div className="file-card-size">
                                {fileStates[msg.file_id]?.status === 'downloading'
                                  ? '下载中...'
                                  : fileStates[msg.file_id]?.status === 'done'
                                  ? '已下载 · 双击打开'
                                  : formatFileSize(msg.file_size)
                                }
                              </div>
                            )}
                          </div>
                          <span className="file-card-action">
                            {fileStates[msg.file_id]?.status === 'done' ? (
                              <span className="file-card-open-icon">📂</span>
                            ) : fileStates[msg.file_id]?.status === 'downloading' ? (
                              <span className="file-card-spinner">⏳</span>
                            ) : (
                              <span className="file-card-download-icon">⬇️</span>
                            )}
                          </span>
                          {fileStates[msg.file_id]?.status === 'downloading' && (
                            <div className="file-card-progress">
                              <div className="file-card-progress-bar" />
                            </div>
                          )}
                        </div>
                      )}

                      {/* Image message */}
                      {msg.message_type === "image" && msg.file_id && (
                        <div
                          className="image-thumb"
                          onClick={() => handleImageClick(msg)}
                        >
                          <ChatImage fileId={msg.file_id} fileName={msg.file_name || undefined} />
                        </div>
                      )}

                      <div className="chat-msg-time">{formatTime(msg.timestamp)}</div>
                    </div>
                  </div>
                );
              })}
            </div>
          ))}
          <div ref={messagesEndRef} />
        </div>

        {/* Input Area */}
        <div className="chat-input-area">
          <button
            className="chat-tool-btn"
            onClick={handleFileSelect}
            title="发送文件"
          >
            📎
          </button>
          <div className="chat-input-wrapper">
            <textarea
              className="chat-input"
              placeholder="输入消息..."
              value={inputText}
              onChange={(e) => setInputText(e.target.value)}
              onKeyDown={handleKeyDown}
              rows={1}
            />
          </div>
          <button
            className="chat-tool-btn"
            onClick={() => setShowEmojiPicker(!showEmojiPicker)}
            title="表情"
          >
            😊
          </button>
          <button
            className="btn btn-primary chat-send-btn"
            onClick={handleSend}
            disabled={!inputText.trim()}
          >
            发送
          </button>
        </div>

        {/* Emoji Picker */}
        {showEmojiPicker && (
          <div className="emoji-picker" ref={emojiPickerRef}>
            {EMOJI_LIST.map((emoji, i) => (
              <button
                key={i}
                className="emoji-item"
                onClick={() => handleEmojiClick(emoji)}
              >
                {emoji}
              </button>
            ))}
          </div>
        )}
      </div>

      ) : (
        <div className="chat-area chat-area-empty">
          <div className="chat-empty">
            <p>请选择好友开始聊天</p>
            <p className="hint">从左侧好友列表选择一个好友</p>
          </div>
        </div>
      )}

      {/* Image Preview Modal */}
      {previewImage && (
        <div className="modal-overlay image-preview-overlay" onClick={() => setPreviewImage(null)}>
          <div className="image-preview-container">
            <img src={previewImage} alt="预览" className="image-preview-img" />
            <button className="image-preview-close" onClick={() => setPreviewImage(null)}>
              ✕
            </button>
          </div>
        </div>
      )}

      {trustedDeviceList.length === 0 && (
        <div className="friends-empty">
          <p>暂无好友</p>
          <p className="hint">配对成功后，已配对的设备会出现在这里</p>
        </div>
      )}
    </div>
  );
}
