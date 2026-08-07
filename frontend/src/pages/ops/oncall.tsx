import { ProTable } from '@ant-design/pro-components';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { App, Button, Card, DatePicker, Descriptions, Form, Input, Modal, Tag } from 'antd';
import dayjs, { Dayjs } from 'dayjs';
import { useState } from 'react';
import { OncallShiftRow, oncallApi } from '../../services/api';

const FMT = 'YYYY-MM-DD HH:mm:ss';

interface ShiftForm { name: string; assignee: string; range: [Dayjs, Dayjs]; }

export default function OncallPage() {
  const { message } = App.useApp();
  const queryClient = useQueryClient();
  const [open, setOpen] = useState(false);
  const [form] = Form.useForm<ShiftForm>();

  const { data: shifts, isLoading } = useQuery({
    queryKey: ['oncall-shifts'],
    queryFn: () => oncallApi.listShifts(),
  });
  const { data: current } = useQuery({
    queryKey: ['oncall-current'],
    queryFn: () => oncallApi.currentShift(),
    refetchInterval: 60_000,
  });

  const invalidate = () => {
    queryClient.invalidateQueries({ queryKey: ['oncall-shifts'] });
    queryClient.invalidateQueries({ queryKey: ['oncall-current'] });
  };

  const create = useMutation({
    mutationFn: (v: ShiftForm) => oncallApi.createShift({
      name: v.name, assignee: v.assignee,
      start_at: v.range[0].format(FMT), end_at: v.range[1].format(FMT),
    }),
    onSuccess: () => { message.success('班次已创建'); setOpen(false); form.resetFields(); invalidate(); },
    onError: (e: any) => message.error(e.message || '创建失败'),
  });

  const remove = useMutation({
    mutationFn: (id: number) => oncallApi.deleteShift(id),
    onSuccess: () => { message.success('班次已删除'); invalidate(); },
    onError: (e: any) => message.error(e.message || '删除失败'),
  });

  const s = current?.shift;
  const now = dayjs();

  return <>
    <Card style={{ marginBottom: 16 }}>
      <Descriptions title="当前值班" column={2}>
        {s ? <>
          <Descriptions.Item label="班次名称"><Tag color="green">{s.name}</Tag></Descriptions.Item>
          <Descriptions.Item label="值班人"><b>{s.assignee}</b></Descriptions.Item>
          <Descriptions.Item label="开始">{s.start_at}</Descriptions.Item>
          <Descriptions.Item label="结束">{s.end_at}</Descriptions.Item>
          <Descriptions.Item label="剩余时间">
            {s.end_at <= now.format(FMT) ? '已到期' : dayjs(s.end_at).diff(now, 'hour') + ' 小时'}
          </Descriptions.Item>
        </> : <Descriptions.Item label="状态">{current?.message || '当前无生效值班班次'}</Descriptions.Item>}
      </Descriptions>
    </Card>
    <ProTable<OncallShiftRow> rowKey="id" loading={isLoading} search={false}
      dataSource={shifts?.shifts || []} headerTitle="值班排班"
      toolBarRender={() => [<Button key="new" type="primary" onClick={() => setOpen(true)}>新建班次</Button>]}
      columns={[
        { title: '名称', dataIndex: 'name' },
        { title: '值班人', dataIndex: 'assignee' },
        { title: '开始', dataIndex: 'start_at' },
        { title: '结束', dataIndex: 'end_at' },
        { title: '创建时间', dataIndex: 'created_at' },
        { title: '操作', valueType: 'option', render: (_, r) => [
          <a key="del" onClick={() => remove.mutate(r.id)}>删除</a>,
        ] },
      ]} />
    <Modal title="新建值班班次" open={open} onCancel={() => setOpen(false)}
      onOk={() => form.submit()} confirmLoading={create.isPending} destroyOnClose>
      <Form form={form} layout="vertical" onFinish={(v) => create.mutate(v)}>
        <Form.Item name="name" label="班次名称" rules={[{ required: true, max: 128 }]}>
          <Input placeholder="如 晚班 / 周末值班" />
        </Form.Item>
        <Form.Item name="assignee" label="值班人" rules={[{ required: true, max: 64 }]}>
          <Input placeholder="姓名或团队" />
        </Form.Item>
        <Form.Item name="range" label="值班时间" rules={[{ required: true, message: '请选择起止时间' }]}>
          <DatePicker.RangePicker showTime format={FMT} style={{ width: '100%' }} />
        </Form.Item>
      </Form>
    </Modal>
  </>;
}
