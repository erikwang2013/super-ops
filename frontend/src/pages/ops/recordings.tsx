import { ProTable } from '@ant-design/pro-components';
import { Button, Modal, Popconfirm, Tag, message } from 'antd';
import { CaretRightOutlined, PauseOutlined, DeleteOutlined } from '@ant-design/icons';
import { useEffect, useRef, useState } from 'react';
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query';
import { recordingApi, RecordingRow, FrameRow } from '../../services/api';

function fmtTime(unixSec: number): string {
  if (!unixSec) return '-';
  return new Date(unixSec * 1000).toLocaleString();
}

function b64ToText(b64: string): string {
  try {
    const bin = atob(b64);
    const bytes = Uint8Array.from(bin, (c) => c.charCodeAt(0));
    return new TextDecoder('utf-8').decode(bytes);
  } catch {
    return `（帧解码失败）${b64.slice(0, 40)}`;
  }
}

const PLAY_INTERVAL_MS = 120;

export default function RecordingsPage() {
  const qc = useQueryClient();
  const [playing, setPlaying] = useState<RecordingRow | null>(null);
  const [frames, setFrames] = useState<FrameRow[]>([]);
  const [cursor, setCursor] = useState(0);
  const [paused, setPaused] = useState(false);
  const timer = useRef<number | null>(null);

  const { data, isLoading, error } = useQuery({
    queryKey: ['recordings'],
    queryFn: () => recordingApi.listRecordings(),
  });

  useEffect(() => () => { if (timer.current) window.clearInterval(timer.current); }, []);

  const openReplay = async (r: RecordingRow) => {
    setPlaying(r); setCursor(0); setPaused(false); setFrames([]);
    try {
      const res = await recordingApi.getFrames(r.session_id);
      const sorted = [...res.frames].sort((a, b) => a.seq - b.seq);
      setFrames(sorted);
    } catch (e) {
      message.error((e as Error).message);
    }
  };

  const stopTimer = () => { if (timer.current) { window.clearInterval(timer.current); timer.current = null; } };

  const startPlay = () => {
    stopTimer();
    setPaused(false);
    timer.current = window.setInterval(() => {
      setCursor((c) => {
        if (c >= frames.length) { stopTimer(); return c; }
        return c + 1;
      });
    }, PLAY_INTERVAL_MS);
  };

  const togglePause = () => {
    if (paused) { startPlay(); } else { stopTimer(); setPaused(true); }
  };

  const remove = useMutation({
    mutationFn: (sid: string) => recordingApi.deleteRecording(sid),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['recordings'] }); message.success('已删除'); },
    onError: (e: Error) => message.error(e.message),
  });

  const columns = [
    { title: '会话 ID', dataIndex: 'session_id', ellipsis: true },
    { title: '开始时间', dataIndex: 'created_at', width: 180, render: (_: unknown, r: RecordingRow) => fmtTime(r.created_at) },
    { title: '帧数', dataIndex: 'frames', width: 80 },
    { title: '操作', width: 160, render: (_: unknown, r: RecordingRow) => (
      <>
        <Button type="link" size="small" icon={<CaretRightOutlined />} onClick={() => openReplay(r)}>回放</Button>
        <Popconfirm title="确定删除该录制？" onConfirm={() => remove.mutate(r.session_id)}>
          <Button type="link" danger size="small" icon={<DeleteOutlined />}>删除</Button>
        </Popconfirm>
      </>
    )},
  ];

  const shown = frames.slice(0, Math.max(cursor, frames.length));

  return <>
    <ProTable<RecordingRow> rowKey="session_id" search={false} loading={isLoading}
      dataSource={data?.recordings || []}
      columns={columns as never}
      headerTitle="终端录制（exec_session，最近 50 条）"
      locale={{ emptyText: error ? `查询失败：${error.message}` : '暂无录制' }}
      options={false} />
    <Modal title={`回放: ${playing?.session_id || ''}`} open={!!playing} width={720} footer={null}
      onCancel={() => { setPlaying(null); stopTimer(); }}>
      <div style={{ marginBottom: 8 }}>
        <Tag>{shown.length}/{frames.length} 帧</Tag>
        <Button size="small" icon={<CaretRightOutlined />} style={{ marginRight: 8 }}
          onClick={startPlay} disabled={paused || cursor >= frames.length}>播放</Button>
        <Button size="small" icon={<PauseOutlined />} onClick={togglePause} disabled={!timer.current}>暂停</Button>
        <Button size="small" type="link" onClick={() => { stopTimer(); setCursor(frames.length); }}>显示全部</Button>
      </div>
      <pre style={{ whiteSpace: 'pre-wrap', maxHeight: 480, overflow: 'auto', background: '#1e1e1e', color: '#d4d4d4', padding: 12, borderRadius: 6 }}>
        {shown.map((f) => b64ToText(f.data)).join('') || '（无帧内容）'}
      </pre>
    </Modal>
  </>;
}
