import { StatisticCard } from '@ant-design/pro-components';
import { Row, Col, Card, List } from 'antd';
import { CloudServerOutlined, ContainerOutlined, GithubOutlined, WarningOutlined } from '@ant-design/icons';

export default function Dashboard() {
  return (
    <div>
      <Row gutter={[16, 16]}>
        <Col span={6}><StatisticCard statistic={{ title: 'K8s 集群', value: 0, icon: <CloudServerOutlined /> }} /></Col>
        <Col span={6}><StatisticCard statistic={{ title: 'Docker 主机', value: 0, icon: <ContainerOutlined /> }} /></Col>
        <Col span={6}><StatisticCard statistic={{ title: 'Pipeline', value: 0, icon: <GithubOutlined /> }} /></Col>
        <Col span={6}><StatisticCard statistic={{ title: '活跃告警', value: 0, icon: <WarningOutlined /> }} /></Col>
      </Row>
      <Row gutter={[16, 16]} style={{ marginTop: 16 }}>
        <Col span={12}><Card title="集群健康"><List dataSource={[]} locale={{ emptyText: '暂无集群' }} renderItem={(i: any) => <List.Item>{i.name}</List.Item>} /></Card></Col>
        <Col span={12}><Card title="最近告警"><List dataSource={[]} locale={{ emptyText: '暂无告警' }} renderItem={(i: any) => <List.Item><List.Item.Meta title={i.message} description={i.time} /></List.Item>} /></Card></Col>
      </Row>
    </div>
  );
}
