mime type 구현
```kdl
// 기본 MIME 매핑 오버라이드
mimetypes {
    xyz "chemical/x-xyz" // 아까 그 '이색히' 추가
}

// 특정 조건에 따른 헤더 추가
headers {
    match path="/static/*" {
        Cache-Control "max-age=86400"
    }
}
```

개별 라우트: 필요한 곳만 명시적으로 켬
```kdl
route "/pub/linux" {
    root "/mnt/storage/linux"
    directory_listing true  // 전역 설정을 무시하고 활성화
}

route "/private" {
    root "/home/user/secret"
    // 명시하지 않으면 전역 설정(false)을 따름
}
```
