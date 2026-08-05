import { api } from './api';

export interface Cluster { id: string; name: string; version: string; node_count: number; pod_count: number; status: string; }
export interface Pod { name: string; namespace: string; status: string; node: string; restarts: number; age: string; }
export interface Deployment { name: string; namespace: string; replicas: number; ready_replicas: number; age: string; }
export interface NodeInfo { name: string; status: string; role: string; version: string; cpu: string; memory: string; }

export const k8sApi = {
  listClusters: () => api.get<{ clusters: Cluster[] }>('/k8s/clusters'),
  addCluster: (name: string, kubeconfig: string) => api.post<{ id: string; name: string; status: string }>('/k8s/clusters', { name, kubeconfig }),
  removeCluster: (id: string) => api.delete(`/k8s/clusters/${id}`),
  listPods: (clusterId: string, namespace?: string) => api.get<{ pods: Pod[] }>(`/k8s/clusters/${clusterId}/pods?namespace=${namespace || ''}`),
  listDeployments: (clusterId: string) => api.get<{ deployments: Deployment[] }>(`/k8s/clusters/${clusterId}/deployments`),
  listNodes: (clusterId: string) => api.get<{ nodes: NodeInfo[] }>(`/k8s/clusters/${clusterId}/nodes`),
};
