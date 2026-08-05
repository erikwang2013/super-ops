.PHONY: proto gateway k8s-service frontend dev clean dev-all stop-all

proto:
	cd protos && buf generate

gateway:
	cd services/gateway && cargo build

k8s-service:
	cd services/k8s && cargo build

frontend:
	cd frontend && npm run dev

dev: proto
	$(MAKE) gateway && $(MAKE) k8s-service

dev-all:
	cd deploy && docker compose up -d
	sleep 5
	cd services/gateway && cargo run &
	sleep 2
	cd services/k8s && cargo run &
	sleep 2
	cd frontend && npm run dev

stop-all:
	cd deploy && docker compose down
	pkill -f "superops-gateway" || true
	pkill -f "superops-k8s" || true

clean:
	cargo clean
	rm -rf frontend/dist frontend/node_modules
