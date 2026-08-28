import { useState } from 'react';
import { LoginFormPage, ProFormText } from '@ant-design/pro-components';
import { message } from 'antd';
import { UserOutlined, LockOutlined } from '@ant-design/icons';
import { api } from '../services/api';
import { useAuthStore } from '../stores/auth';

interface LoginResponse { access_token: string; refresh_token: string; expires_in: number; }

export default function LoginPage() {
  const [loading, setLoading] = useState(false);
  const login = useAuthStore((s) => s.login);

  const handleSubmit = async (v: { username: string; password: string }) => {
    setLoading(true);
    try {
      const res = await api.post<LoginResponse>('/auth/login', v);
      login(res.access_token, v.username, res.refresh_token); message.success('登录成功');
    } catch (e: any) { message.error(e.message || '登录失败'); }
    finally { setLoading(false); }
  };

  return (
    <LoginFormPage title="SuperOps" subTitle="超级运维系统" onFinish={handleSubmit} loading={loading}
      submitter={{ searchConfig: { submitText: '登录' } }}>
      <ProFormText name="username" fieldProps={{ size: 'large', prefix: <UserOutlined /> }} placeholder="用户名" rules={[{ required: true }]} />
      <ProFormText.Password name="password" fieldProps={{ size: 'large', prefix: <LockOutlined /> }} placeholder="密码" rules={[{ required: true }]} />
    </LoginFormPage>
  );
}
