# 多云接入指南

SuperOps 支持接入多个 Kubernetes 集群（可来自不同云厂商或本地环境），
统一在「Kubernetes → 集群管理」页维护，全部资源操作按 `cluster_id` 路由到对应集群。

## 架构

```
前端 (集群管理页/聚合视图)
   │ HTTP
   ▼
gateway (8080)  ──gRPC──►  k8s-service (9091)
                             │ ClusterManager（内存，按 cluster_id 路由）
                             ├─ Cluster A (云厂商 A / kubeconfig)
                             ├─ Cluster B (云厂商 B / kubeconfig)
                             └─ Cluster C (本地 k3s / kubeconfig)
```

- `gateway` 是转发层：`/api/k8s/*` 全部转发给 k8s-service 对应 gRPC 方法。
- `k8s-service` 的 `ClusterManager` 以内存保存已注册集群客户端，重启后需重新注册。
- 每个集群用标准 kubeconfig（支持任意云厂商，仅需集群端点与凭据）。

## 注册集群

### 方式一：前端页面

「Kubernetes → 集群管理」→ 新增集群 → 粘贴 kubeconfig（YAML 文本）→ 保存。

### 方式二：API

```bash
curl -X POST http://<gateway>:8080/api/k8s/clusters \
  -H "Authorization: Bearer <token>" \
  -d '{
    "name": "aliyun-sh",
    "kubeconfig": "apiVersion: v1\nkind: Config\n..."
  }'
# → {"id":"<uuid>","name":"aliyun-sh","status":"OK"}
```

要点：

- `name` 为展示名；`kubeconfig` 为集群访问凭据，gateway 原样转发给 k8s-service。
- 注册成功即返回 `id`，后续所有资源操作使用该 `id`。
- kubeconfig 即凭证：勿经公开渠道传输，勿提交进 git。

## 查询与操作（按集群）

| 操作 | 端点 |
|------|------|
| 列出集群 | `GET /api/k8s/clusters` |
| 集群详情 | `GET /api/k8s/clusters/{cluster_id}` |
| 删除集群 | `DELETE /api/k8s/clusters/{cluster_id}` |
| 节点列表 | `GET /api/k8s/clusters/{cluster_id}/nodes` |
| Pod 列表 | `GET /api/k8s/clusters/{cluster_id}/pods?namespace=` |
| Deployment 列表 | `GET /api/k8s/clusters/{cluster_id}/deployments` |
| Pod 日志 | `GET /api/k8s/clusters/{cluster_id}/pods/{namespace}/{pod}/logs` |
| 镜像更新 | `POST /api/k8s/clusters/{cluster_id}/deployments/{name}/image` |
| 终端会话 | `WS /api/k8s/clusters/{cluster_id}/pods/{namespace}/{pod}/exec?container=&confirm=1` |

## 跨集群聚合视图

总览页「集群健康」与 `GET /api/k8s/aggregate` 提供全集群汇总：

```json
{
  "clusters": [
    {
      "id": "uuid-1", "name": "aliyun-sh",
      "version": "v1.30.2", "status": "Ready",
      "node_count": 3, "nodes_ready": 3,
      "pod_count": 12, "pods_running": 11
    }
  ],
  "totals": { "clusters": 1, "nodes": 3, "nodes_ready": 3, "pods": 12, "pods_running": 11 }
}
```

- 聚合为顺序遍历：每集群分别拉取节点与 Pod 后汇总。
- `nodes_ready` 口径：节点 `status == "Ready"`；`pods_running` 口径：Pod `status == "Running"`。
- 单个集群后端不可达时该集群仍会返回（无节点/Pod 计数），可据此快速定位异常集群。

## 告警与巡检

collector 的 k8s 巡检端点（`collector.yaml → k8s.endpoint`）指向 k8s-service，
可对单集群持续巡检；多集群告警聚合可通过接入多个 collector 实例或后续扩展实现。
