import { Button, Card, Input, Space, Upload, message } from 'antd';
import { InboxOutlined, DownloadOutlined, FileOutlined } from '@ant-design/icons';
import { useState } from 'react';
import { fileApi } from '../../services/api';

export default function FilesPage() {
  const [uploaded, setUploaded] = useState<string[]>([]);
  const [downloadName, setDownloadName] = useState('');
  const [downloading, setDownloading] = useState(false);

  const doUpload = async (file: File) => {
    try {
      const res = await fileApi.upload(file);
      setUploaded((prev) => (prev.includes(res.name) ? prev : [res.name, ...prev]));
      message.success(`已上传 ${res.name}`);
    } catch (e) {
      message.error((e as Error).message);
    }
    return false;
  };

  const doDownload = async (name: string) => {
    setDownloading(true);
    try {
      const blob = await fileApi.download(name);
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url; a.download = name;
      document.body.appendChild(a);
      a.click();
      a.remove();
      URL.revokeObjectURL(url);
      message.success('已开始下载');
    } catch (e) {
      message.error((e as Error).message);
    } finally {
      setDownloading(false);
    }
  };

  return <Space direction="vertical" size={16} style={{ width: '100%' }}>
    <Card title="上传文件" size="small">
      <Upload.Dragger multiple={false} showUploadList={false} beforeUpload={doUpload}
        accept="*" maxCount={1}>
        <p className="ant-upload-drag-icon"><InboxOutlined /></p>
        <p className="ant-upload-text">点击或拖拽文件到此区域上传</p>
        <p className="ant-upload-hint">单文件 1B ~ 10MB；文件名仅限字母/数字/._-（1-255 字符），存储于 MinIO</p>
      </Upload.Dragger>
    </Card>
    <Card title="本会话已上传" size="small" style={{ width: '100%' }}>
      {uploaded.length === 0
        ? <span style={{ color: '#999' }}>（暂无，上传后此处可快速下载）</span>
        : uploaded.map((name) => (
          <Space key={name} style={{ marginBottom: 8, width: '100%', justifyContent: 'space-between' }}>
            <span><FileOutlined style={{ marginRight: 6 }} />{name}</span>
            <Button size="small" icon={<DownloadOutlined />} onClick={() => doDownload(name)}>下载</Button>
          </Space>
        ))}
    </Card>
    <Card title="下载文件" size="small" style={{ width: '100%' }}>
      <Space.Compact style={{ width: '100%' }}>
        <Input placeholder="输入文件名（需已通过本页或 API 上传）"
          value={downloadName}
          onChange={(e) => setDownloadName(e.target.value)}
          onPressEnter={() => downloadName.trim() && doDownload(downloadName.trim())} />
        <Button type="primary" icon={<DownloadOutlined />} loading={downloading}
          onClick={() => downloadName.trim() && doDownload(downloadName.trim())}>
          下载
        </Button>
      </Space.Compact>
    </Card>
  </Space>;
}
